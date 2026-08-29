import assert from 'node:assert/strict';
import { once } from 'node:events';
import { existsSync } from 'node:fs';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

import {
  formatToolResult,
  invokeTool,
  processToolResponse,
  redactSecrets,
  tokenNeedsRefresh,
} from './kimi-datasource.mjs';

const scriptPath = fileURLToPath(new URL('./kimi-datasource.mjs', import.meta.url));

async function temporaryDirectory() {
  return mkdtemp(path.join(tmpdir(), 'kimi-datasource-test-'));
}

async function killAndWait(child) {
  if (child.exitCode !== null || child.signalCode !== null) return;
  const exited = once(child, 'exit');
  if (!child.kill('SIGKILL')) throw new Error('failed to kill child process');
  await exited;
}

async function runCliWithStdin(args, input, environment = {}) {
  const child = spawn(process.execPath, [scriptPath, ...args], {
    env: { ...process.env, ...environment },
    stdio: ['pipe', 'pipe', 'pipe'],
  });
  const stdout = [];
  const stderr = [];
  child.stdout.on('data', (chunk) => stdout.push(chunk));
  child.stderr.on('data', (chunk) => stderr.push(chunk));
  child.stdin.on('error', () => {});
  const closed = once(child, 'close');
  let timer;
  try {
    child.stdin.end(input);
    const [code, signal] = await Promise.race([
      closed,
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error('CLI child timed out')), 2_000);
      }),
    ]);
    return {
      code,
      signal,
      stdout: Buffer.concat(stdout).toString('utf8'),
      stderr: Buffer.concat(stderr).toString('utf8'),
    };
  } finally {
    clearTimeout(timer);
    await killAndWait(child);
  }
}

test('token refresh decisions use the expiry threshold deterministically', () => {
  const now = 1_000;

  assert.equal(tokenNeedsRefresh({ expires_at: now + 301 }, false, now), false);
  assert.equal(tokenNeedsRefresh({ expires_at: now + 299 }, false, now), true);
  assert.equal(
    tokenNeedsRefresh({ expires_at: now + 499, expires_in: 1_000 }, false, now),
    true,
  );
  assert.equal(tokenNeedsRefresh({ expires_at: 0 }, false, now), false);
  assert.equal(tokenNeedsRefresh({ expires_at: now + 10_000 }, true, now), true);
});

test('secret redaction covers raw, encoded, field, and bearer forms', () => {
  const secret = 'token/value';
  const output = redactSecrets(
    `raw=${secret} encoded=${encodeURIComponent(secret)} access_token=other Bearer abc.def`,
    [secret],
  );

  assert.equal(output.includes(secret), false);
  assert.equal(output.includes(encodeURIComponent(secret)), false);
  assert.match(output, /access_token=\[REDACTED\]/u);
  assert.match(output, /Bearer \[REDACTED\]/u);
});

test('failed tool envelopes cannot commit returned files', async () => {
  const directory = await temporaryDirectory();
  const outputPath = path.join(directory, 'result.txt');
  try {
    await assert.rejects(
      processToolResponse(
        {
          is_success: false,
          error: { assistant: [{ type: 'text', text: 'backend rejected the request' }] },
          files: [{ name: outputPath, content: 'must not be written' }],
        },
        {
          method: 'call_data_source_tool',
          params: { output_path: outputPath },
        },
      ),
      /backend rejected the request/u,
    );
    assert.equal(existsSync(outputPath), false);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test('successful tool envelopes commit only requested files', async () => {
  const directory = await temporaryDirectory();
  const outputPath = path.join(directory, 'result.txt');
  const siblingPath = path.join(directory, 'result_a.txt');
  const unexpectedPath = path.join(directory, 'unexpected.txt');
  const outsidePath = path.join(directory, 'outside', 'result.txt');
  try {
    const result = await processToolResponse(
      {
        is_success: true,
        result: { assistant: [{ type: 'text', text: 'completed' }] },
        files: [
          { name: outputPath, content: 'persisted result' },
          { name: siblingPath, content: 'persisted sibling' },
          { name: unexpectedPath, content: 'must not be written' },
          { name: outsidePath, content: 'must not be written' },
        ],
      },
      {
        method: 'call_data_source_tool',
        params: { output_path: outputPath },
      },
    );

    assert.match(result.text, new RegExp(`Skipped returned file.*${path.basename(unexpectedPath)}`, 'u'));
    assert.match(result.text, new RegExp(`Skipped returned file.*${path.basename(outsidePath)}`, 'u'));
    assert.deepEqual(result.writtenFiles, [outputPath, siblingPath]);
    assert.equal(await readFile(outputPath, 'utf8'), 'persisted result');
    assert.equal(await readFile(siblingPath, 'utf8'), 'persisted sibling');
    assert.equal(existsSync(unexpectedPath), false);
    assert.equal(existsSync(outsidePath), false);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test('gateway 401 triggers one forced refresh and one retry', async () => {
  const refreshCalls = [];
  const gatewayCalls = [];
  const attempts = [
    {
      response: { status: 401, ok: false },
      text: 'unauthorized',
      toolCallId: 'call-1',
      requestId: undefined,
    },
    {
      response: { status: 200, ok: true },
      text: JSON.stringify({
        is_success: true,
        result: { assistant: [{ type: 'text', text: 'retried response' }] },
      }),
      toolCallId: 'call-2',
      requestId: 'request-2',
    },
  ];
  const result = await invokeTool(
    'get_data_source_desc',
    { name: 'arxiv' },
    {
      ensureFreshAccessToken: async (options = {}) => {
        refreshCalls.push(options);
        return {
          kimiHome: '/tmp/kimi-test-home',
          token: options.force === true ? 'new-token' : 'old-token',
          refreshed: options.force === true,
        };
      },
      gatewayRequest: async (...args) => {
        gatewayCalls.push(args);
        return attempts.shift();
      },
      loadTokenWire: async () => ({ access_token: 'new-token', refresh_token: 'refresh-token' }),
      writeResponseFiles: async () => ({ warnings: [], writtenFiles: [] }),
    },
  );

  assert.deepEqual(refreshCalls, [{}, { force: true }]);
  assert.equal(gatewayCalls.length, 2);
  assert.deepEqual(gatewayCalls.map((call) => call[2].token), ['old-token', 'new-token']);
  assert.equal(result.trace.refreshed, true);
  assert.equal(result.trace.requestId, 'request-2');
  assert.match(result.text, /^retried response/u);
});

test('a repeated gateway 401 stops after the single forced retry', async () => {
  const refreshCalls = [];
  const gatewayCalls = [];
  await assert.rejects(
    invokeTool(
      'get_data_source_desc',
      { name: 'arxiv' },
      {
        ensureFreshAccessToken: async (options = {}) => {
          refreshCalls.push(options);
          return {
            kimiHome: '/tmp/kimi-test-home',
            token: options.force === true ? 'new-token' : 'old-token',
            refreshed: options.force === true,
          };
        },
        gatewayRequest: async (...args) => {
          gatewayCalls.push(args);
          return {
            response: { status: 401, ok: false },
            text: gatewayCalls.length === 1 ? 'first unauthorized' : 'second unauthorized',
            toolCallId: `call-${gatewayCalls.length}`,
            requestId: undefined,
          };
        },
        loadTokenWire: async () => ({
          access_token: 'new-token',
          refresh_token: 'refresh-token',
        }),
      },
    ),
    /HTTP 401: second unauthorized/u,
  );

  assert.deepEqual(refreshCalls, [{}, { force: true }]);
  assert.equal(gatewayCalls.length, 2);
  assert.deepEqual(gatewayCalls.map((call) => call[2].token), ['old-token', 'new-token']);
});

test('tool output formatting preserves text, json, and quiet contracts', () => {
  const result = {
    text: 'result\n\n[kimi-datasource] request-id: request · tool-call-id: call',
    writtenFiles: ['/tmp/result'],
    trace: { requestId: 'request', toolCallId: 'call', refreshed: false },
  };

  assert.match(formatToolResult(result, { output: 'text' }, {}), /tool-call-id/u);
  assert.equal(formatToolResult(result, { output: 'text', quiet: true }, {}), 'result');
  assert.deepEqual(
    JSON.parse(formatToolResult(result, { output: 'json', quiet: true }, { command: 'desc' })),
    {
      ok: true,
      command: 'desc',
      text: 'result',
      files: ['/tmp/result'],
    },
  );
});

test('call reads valid and invalid JSON objects from stdin', { timeout: 5_000 }, async () => {
  const kimiHome = await temporaryDirectory();
  try {
    const valid = await runCliWithStdin(
      ['call', 'arxiv', 'search', '--json-file', '-'],
      '{"query":"retrieval"}\n',
      { KIMI_CODE_HOME: kimiHome },
    );
    assert.equal(valid.code, 1, valid.stderr);
    assert.match(valid.stderr, /OAuth credentials are unavailable/u);
    assert.doesNotMatch(valid.stderr, /ERR_INVALID_ARG_TYPE/u);

    const invalid = await runCliWithStdin(
      ['call', 'arxiv', 'search', '--json-file', '-'],
      '{invalid}\n',
      { KIMI_CODE_HOME: kimiHome },
    );
    assert.equal(invalid.code, 1, invalid.stderr);
    assert.match(invalid.stderr, /Invalid JSON from stdin/u);
    assert.doesNotMatch(invalid.stderr, /OAuth credentials are unavailable/u);
  } finally {
    await rm(kimiHome, { recursive: true, force: true });
  }
});
