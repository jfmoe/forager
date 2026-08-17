#!/usr/bin/env node

/*
 * Portions adapted from MoonshotAI/kimi-code and
 * Demogorgon314/kimi-datasource.
 *
 * MIT License
 *
 * Copyright (c) 2026 Moonshot AI
 * Copyright (c) 2026 kimi-datasource contributors
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 */

import { createHash, randomBytes, randomUUID } from 'node:crypto';
import { chmod, mkdir, open, readFile, rename, rm } from 'node:fs/promises';
import { arch, homedir, hostname, release, type } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

export const VERSION = '3.3.0';
export const DATA_SOURCE_NAMES = [
  'stock_finance_data',
  'yahoo_finance',
  'world_bank_open_data',
  'tianyancha',
  'arxiv',
  'scholar',
  'yuandian_law',
  'wind',
  'imf',
  'gildata',
  'sec_edgar',
  'sp_data',
];

const DEFAULT_KIMI_CODE_OAUTH_HOST = 'https://auth.kimi.com';
const DEFAULT_KIMI_CODE_BASE_URL = 'https://api.kimi.com/coding/v1';
const DEFAULT_KIMI_CODE_CLIENT_ID = '17e5f671-d194-4dfb-9706-5516cb48c098';
const DEFAULT_TIMEOUT_MS = 30_000;
const DEFAULT_REFRESH_THRESHOLD_SECONDS = 300;
const KIMI_CODE_PLATFORM = 'kimi_code_cli';

const GLOBAL_HELP = `kimi-datasource v${VERSION}

Usage:
  kimi-datasource <command> [args] [options]

Commands:
  status                        Show Kimi Code OAuth credential status
  refresh                       Force-refresh the Kimi Code OAuth token
  sources                       List the 12 official data sources
  desc <source>                 Fetch current API documentation
  call <source> <api>           Call one data-source API

Options:
  --output <text|json>          Output format (default: text)
  --quiet, -q                   Omit request trace metadata
  --help, -h                    Show help

Run "kimi-datasource call --help" for call parameter options.
`;

const CALL_HELP = `Usage:
  kimi-datasource call <source> <api> [--json <object> | --json-file <path|->] [options]

Options:
  --json <object>               Params as an inline JSON object
  --json-file <path|->          Params as a JSON object from a file or stdin
  --output <text|json>          Output format (default: text)
  --quiet, -q                   Omit request trace metadata
  --help, -h                    Show help
`;

const CLI_OPTIONS = {
  help: { type: 'boolean', short: 'h' },
  version: { type: 'boolean', short: 'V' },
  quiet: { type: 'boolean', short: 'q' },
  json: { type: 'string' },
  'json-file': { type: 'string' },
  output: { type: 'string' },
};

const COMMAND_FLAGS = {
  status: new Set(['quiet', 'output']),
  refresh: new Set(['quiet', 'output']),
  sources: new Set(['quiet', 'output']),
  desc: new Set(['quiet', 'output']),
  call: new Set(['quiet', 'json', 'json-file', 'output']),
};

function assertSupportedNode() {
  const [major, minor] = process.versions.node.split('.').map(Number);
  if (major < 18 || (major === 18 && minor < 17)) {
    throw new Error(`Node.js >= 18.17 is required; found ${process.versions.node}.`);
  }
}

function normalizeEndpoint(value) {
  return String(value).trim().replace(/\/+$/u, '');
}

function kimiCodeBaseUrl() {
  return normalizeEndpoint(process.env.KIMI_CODE_BASE_URL ?? DEFAULT_KIMI_CODE_BASE_URL);
}

function kimiCodeOAuthHost() {
  return normalizeEndpoint(
    process.env.KIMI_CODE_OAUTH_HOST ??
      process.env.KIMI_OAUTH_HOST ??
      DEFAULT_KIMI_CODE_OAUTH_HOST,
  );
}

function kimiCodeClientId() {
  return process.env.KIMI_CODE_CLIENT_ID?.trim() || DEFAULT_KIMI_CODE_CLIENT_ID;
}

function datasourceApiUrl() {
  return process.env.KIMI_DATASOURCE_API_URL?.trim() || `${kimiCodeBaseUrl()}/tools`;
}

function requestTimeoutMs() {
  const raw = process.env.KIMI_DATASOURCE_TIMEOUT_MS?.trim();
  if (raw === undefined || raw.length === 0) return DEFAULT_TIMEOUT_MS;
  const parsed = Number(raw);
  if (!Number.isInteger(parsed) || parsed <= 0) {
    throw new Error('KIMI_DATASOURCE_TIMEOUT_MS must be a positive integer.');
  }
  return parsed;
}

export function resolveKimiHome() {
  const explicit = process.env.KIMI_CODE_HOME?.trim();
  return explicit && explicit.length > 0 ? explicit : path.join(homedir(), '.kimi-code');
}

export function resolveCredentialName() {
  const oauthHost = kimiCodeOAuthHost();
  const baseUrl = kimiCodeBaseUrl();
  if (
    oauthHost === normalizeEndpoint(DEFAULT_KIMI_CODE_OAUTH_HOST) &&
    baseUrl === normalizeEndpoint(DEFAULT_KIMI_CODE_BASE_URL)
  ) {
    return 'kimi-code';
  }

  const digest = createHash('sha256')
    .update(JSON.stringify({ oauthHost, baseUrl }))
    .digest('hex')
    .slice(0, 16);
  return `kimi-code-env-${digest}`;
}

export function credentialsPath(kimiHome = resolveKimiHome()) {
  return path.join(kimiHome, 'credentials', `${resolveCredentialName()}.json`);
}

function refreshLockPath(kimiHome = resolveKimiHome()) {
  return path.join(kimiHome, 'oauth', `${resolveCredentialName()}.lock`);
}

function isRecord(value) {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isNotFound(error) {
  return isRecord(error) && error.code === 'ENOENT';
}

function asciiHeader(value, fallback = 'unknown') {
  const cleaned = String(value).replaceAll(/[^ -~]/gu, '').trim();
  return cleaned.length > 0 ? cleaned : fallback;
}

function delay(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

async function atomicWritePrivate(filePath, contents) {
  const directory = path.dirname(filePath);
  await mkdir(directory, { recursive: true, mode: 0o700 });
  const temporaryPath = path.join(
    directory,
    `.${path.basename(filePath)}.tmp.${process.pid}.${randomBytes(6).toString('hex')}`,
  );
  let handle;
  try {
    handle = await open(temporaryPath, 'wx', 0o600);
    await handle.writeFile(contents);
    await handle.sync();
    await handle.close();
    handle = undefined;
    await chmod(temporaryPath, 0o600);
    await rename(temporaryPath, filePath);
  } catch (error) {
    if (handle !== undefined) await handle.close().catch(() => {});
    await rm(temporaryPath, { force: true }).catch(() => {});
    throw error;
  }
}

export async function loadTokenWire(filePath = credentialsPath()) {
  let raw;
  try {
    raw = await readFile(filePath, 'utf8');
  } catch (error) {
    if (isNotFound(error)) return undefined;
    throw error;
  }

  let parsed;
  try {
    parsed = JSON.parse(raw);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new Error(`Invalid Kimi Code credentials JSON at ${filePath}: ${message}`);
  }
  if (!isRecord(parsed)) {
    throw new Error(`Invalid Kimi Code credentials object at ${filePath}.`);
  }
  return parsed;
}

export async function saveTokenWire(token, filePath = credentialsPath()) {
  await atomicWritePrivate(filePath, `${JSON.stringify(token, null, 2)}\n`);
}

function classifyToken(token) {
  if (token === undefined) return { kind: 'missing' };
  if (typeof token.access_token !== 'string' || token.access_token.length === 0) {
    return { kind: 'revoked', token };
  }
  return { kind: 'valid', token };
}

function tokenNeedsRefresh(token, force = false) {
  if (force) return true;
  const expiresAt = Number(token.expires_at ?? 0);
  if (!Number.isFinite(expiresAt) || expiresAt === 0) return false;
  const expiresIn = Number(token.expires_in ?? 0);
  const threshold = Math.max(
    DEFAULT_REFRESH_THRESHOLD_SECONDS,
    Number.isFinite(expiresIn) && expiresIn > 0 ? expiresIn * 0.5 : 0,
  );
  return expiresAt - Math.floor(Date.now() / 1000) < threshold;
}

function tokenChanged(left, right) {
  return (
    left.access_token !== right.access_token ||
    left.refresh_token !== right.refresh_token ||
    left.expires_at !== right.expires_at ||
    left.expires_in !== right.expires_in
  );
}

function credentialRecoveryMessage(filePath) {
  return `Kimi Code OAuth credentials are unavailable at ${filePath}. Run /login in Kimi Code.`;
}

async function requireValidToken(filePath = credentialsPath()) {
  const classified = classifyToken(await loadTokenWire(filePath));
  if (classified.kind !== 'valid') throw new Error(credentialRecoveryMessage(filePath));
  return classified.token;
}

async function acquireRefreshLock(kimiHome) {
  if (process.platform === 'win32') return async () => {};

  const lockPath = refreshLockPath(kimiHome);
  await mkdir(path.dirname(lockPath), { recursive: true, mode: 0o700 });
  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) {
    try {
      await mkdir(lockPath, { mode: 0o700 });
      return async () => rm(lockPath, { recursive: true, force: true });
    } catch (error) {
      if (!isRecord(error) || error.code !== 'EEXIST') throw error;
      await delay(50 + Math.floor(Math.random() * 50));
    }
  }
  throw new Error(`Timed out acquiring the Kimi Code OAuth refresh lock: ${lockPath}`);
}

async function createDeviceId(kimiHome) {
  const deviceIdPath = path.join(kimiHome, 'device_id');
  try {
    const existing = (await readFile(deviceIdPath, 'utf8')).trim();
    if (existing.length > 0) return existing;
  } catch {
    // Create one below.
  }

  const id = randomUUID();
  await atomicWritePrivate(deviceIdPath, `${id}\n`).catch(() => {});
  return id;
}

function deviceModel() {
  const osName = type();
  const osVersion = release();
  const osArchitecture = arch();
  if (osName === 'Darwin') return `macOS ${osVersion} ${osArchitecture}`;
  if (osName === 'Windows_NT') return `Windows ${osVersion} ${osArchitecture}`;
  return `${osName} ${osVersion} ${osArchitecture}`.trim();
}

async function deviceHeaders(kimiHome) {
  return {
    'X-Msh-Platform': asciiHeader(process.env.KIMI_MSH_PLATFORM ?? KIMI_CODE_PLATFORM),
    'X-Msh-Version': asciiHeader(process.env.KIMI_MSH_VERSION ?? VERSION),
    'X-Msh-Device-Name': asciiHeader(process.env.KIMI_MSH_DEVICE_NAME ?? hostname()),
    'X-Msh-Device-Model': asciiHeader(process.env.KIMI_MSH_DEVICE_MODEL ?? deviceModel()),
    'X-Msh-Os-Version': asciiHeader(process.env.KIMI_MSH_OS_VERSION ?? release()),
    'X-Msh-Device-Id': asciiHeader(
      process.env.KIMI_MSH_DEVICE_ID ?? (await createDeviceId(kimiHome)),
    ),
  };
}

async function fetchWithTimeout(url, init) {
  const timeoutMs = requestTimeoutMs();
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    return await fetch(url, { ...init, signal: controller.signal });
  } catch (error) {
    if (isRecord(error) && error.name === 'AbortError') {
      throw new Error(`Request timed out after ${timeoutMs}ms.`);
    }
    throw error;
  } finally {
    clearTimeout(timer);
  }
}

function credentialSecrets(token) {
  if (!isRecord(token)) return [];
  return [token.access_token, token.refresh_token].filter(
    (value) => typeof value === 'string' && value.length > 0,
  );
}

export function redactSecrets(value, secrets = []) {
  let output = String(value);
  for (const secret of secrets) {
    if (typeof secret !== 'string' || secret.length === 0) continue;
    output = output.replaceAll(secret, '[REDACTED]');
    const encoded = encodeURIComponent(secret);
    if (encoded !== secret) output = output.replaceAll(encoded, '[REDACTED]');
  }
  output = output.replace(
    /(["']?(?:access_token|refresh_token)["']?\s*[:=]\s*["']?)([^"',\s}&]+)/giu,
    '$1[REDACTED]',
  );
  output = output.replace(/(Bearer\s+)[A-Za-z0-9._~+/=-]+/giu, '$1[REDACTED]');
  return output;
}

function compactResponseBody(body, secrets) {
  const cleaned = redactSecrets(body, secrets).trim();
  return cleaned.length <= 2_000 ? cleaned : `${cleaned.slice(0, 2_000)}…`;
}

function oauthError(message, unauthorized = false) {
  const error = new Error(message);
  if (unauthorized) error.code = 'OAUTH_UNAUTHORIZED';
  return error;
}

async function refreshAccessToken(kimiHome, refreshToken) {
  const url = `${kimiCodeOAuthHost()}/api/oauth/token`;
  const body = new URLSearchParams({
    client_id: kimiCodeClientId(),
    grant_type: 'refresh_token',
    refresh_token: refreshToken,
  }).toString();
  const response = await fetchWithTimeout(url, {
    method: 'POST',
    headers: {
      ...(await deviceHeaders(kimiHome)),
      Accept: 'application/json',
      'Content-Type': 'application/x-www-form-urlencoded',
    },
    body,
  });
  const responseText = await response.text();
  let payload;
  try {
    payload = JSON.parse(responseText);
  } catch {
    payload = {};
  }

  const errorCode = isRecord(payload) && typeof payload.error === 'string' ? payload.error : '';
  if (!response.ok) {
    const detail = compactResponseBody(responseText, [refreshToken]) || 'unknown error';
    const unauthorized =
      response.status === 401 || response.status === 403 || errorCode === 'invalid_grant';
    throw oauthError(`Kimi Code OAuth refresh failed (HTTP ${response.status}): ${detail}`, unauthorized);
  }
  if (!isRecord(payload)) throw new Error('Kimi Code OAuth refresh returned invalid JSON.');

  const accessToken = payload.access_token;
  const rotatedRefreshToken = payload.refresh_token;
  const expiresIn = Number(payload.expires_in);
  if (typeof accessToken !== 'string' || accessToken.length === 0) {
    throw new Error('Kimi Code OAuth refresh response is missing access_token.');
  }
  if (typeof rotatedRefreshToken !== 'string' || rotatedRefreshToken.length === 0) {
    throw new Error('Kimi Code OAuth refresh response is missing refresh_token.');
  }
  if (!Number.isFinite(expiresIn) || expiresIn <= 0) {
    throw new Error('Kimi Code OAuth refresh response has invalid expires_in.');
  }

  return {
    access_token: accessToken,
    refresh_token: rotatedRefreshToken,
    expires_at: Math.floor(Date.now() / 1000) + expiresIn,
    scope: typeof payload.scope === 'string' ? payload.scope : '',
    token_type: typeof payload.token_type === 'string' ? payload.token_type : 'Bearer',
    expires_in: expiresIn,
  };
}

export async function ensureFreshAccessToken(options = {}) {
  const force = options.force === true;
  const kimiHome = resolveKimiHome();
  const filePath = credentialsPath(kimiHome);
  const initial = await requireValidToken(filePath);
  if (!tokenNeedsRefresh(initial, force)) {
    return { kimiHome, token: initial.access_token, refreshed: false };
  }

  const releaseLock = await acquireRefreshLock(kimiHome);
  try {
    const active = await requireValidToken(filePath);
    if (!force && !tokenNeedsRefresh(active, false)) {
      return { kimiHome, token: active.access_token, refreshed: false };
    }
    if (force && tokenChanged(active, initial)) {
      return { kimiHome, token: active.access_token, refreshed: false };
    }
    if (typeof active.refresh_token !== 'string' || active.refresh_token.length === 0) {
      throw new Error(`${credentialRecoveryMessage(filePath)} The stored token has no refresh_token.`);
    }

    try {
      const refreshed = await refreshAccessToken(kimiHome, active.refresh_token);
      await saveTokenWire(refreshed, filePath);
      return { kimiHome, token: refreshed.access_token, refreshed: true };
    } catch (error) {
      if (!isRecord(error) || error.code !== 'OAUTH_UNAUTHORIZED') throw error;

      await delay(100);
      const recovered = classifyToken(await loadTokenWire(filePath));
      if (
        recovered.kind === 'valid' &&
        recovered.token.refresh_token !== active.refresh_token
      ) {
        return { kimiHome, token: recovered.token.access_token, refreshed: true };
      }

      await saveTokenWire(
        {
          access_token: '',
          refresh_token: '',
          expires_at: 0,
          expires_in: 0,
          scope: typeof active.scope === 'string' ? active.scope : '',
          token_type: typeof active.token_type === 'string' ? active.token_type : 'Bearer',
        },
        filePath,
      );
      throw new Error(`${error.message} Run /login in Kimi Code.`);
    }
  } finally {
    await releaseLock();
  }
}

export async function credentialsStatus() {
  const kimiHome = resolveKimiHome();
  const filePath = credentialsPath(kimiHome);
  let token;
  try {
    token = await loadTokenWire(filePath);
  } catch (error) {
    return {
      kind: 'invalid',
      credentialName: resolveCredentialName(),
      filePath,
      error: error instanceof Error ? error.message : String(error),
    };
  }

  const state = classifyToken(token);
  if (state.kind !== 'valid') {
    return { kind: state.kind, credentialName: resolveCredentialName(), filePath };
  }
  const expiresAt = Number(state.token.expires_at ?? 0);
  const remainingSeconds =
    Number.isFinite(expiresAt) && expiresAt > 0
      ? expiresAt - Math.floor(Date.now() / 1000)
      : undefined;
  return {
    kind: remainingSeconds !== undefined && remainingSeconds <= 0 ? 'expired' : 'valid',
    credentialName: resolveCredentialName(),
    filePath,
    expiresAt: Number.isFinite(expiresAt) && expiresAt > 0 ? expiresAt : undefined,
    remainingSeconds,
    needsRefresh: tokenNeedsRefresh(state.token),
    hasRefreshToken:
      typeof state.token.refresh_token === 'string' && state.token.refresh_token.length > 0,
  };
}

function extractRequestId(headers) {
  for (const key of [
    'x-request-id',
    'x-trace-id',
    'x-msh-request-id',
    'x-msh-trace-id',
    'request-id',
  ]) {
    const value = headers.get(key);
    if (typeof value === 'string' && value.trim().length > 0) return value.trim();
  }
  return undefined;
}

async function gatewayRequest(method, params, auth) {
  const toolCallId = randomUUID();
  const response = await fetchWithTimeout(datasourceApiUrl(), {
    method: 'POST',
    headers: {
      Authorization: `Bearer ${auth.token}`,
      'Content-Type': 'application/json',
      'X-Msh-Tool-Call-Id': toolCallId,
      ...(await deviceHeaders(auth.kimiHome)),
      'User-Agent': `kimi-datasource/${VERSION}`,
    },
    body: JSON.stringify({ method, params }),
  });
  return {
    response,
    text: await response.text(),
    toolCallId,
    requestId: extractRequestId(response.headers),
  };
}

function absoluteParameterPaths(value) {
  if (!isRecord(value)) return [];
  return Object.values(value).flatMap((candidate) => {
    if (typeof candidate === 'string' && path.isAbsolute(candidate)) {
      return [path.resolve(candidate)];
    }
    return isRecord(candidate) ? absoluteParameterPaths(candidate) : [];
  });
}

function allowedResponseFilePath(name, parameterPaths) {
  const actual = path.resolve(name);
  const actualParts = path.parse(actual);
  for (const expected of parameterPaths) {
    if (actual === expected) return actual;
    const expectedParts = path.parse(expected);
    if (
      actualParts.dir === expectedParts.dir &&
      actualParts.ext === expectedParts.ext &&
      actualParts.name.startsWith(`${expectedParts.name}_`)
    ) {
      return actual;
    }
  }
  return undefined;
}

async function writeResponseFiles(response, parameterPaths) {
  if (!isRecord(response) || !Array.isArray(response.files)) {
    return { warnings: [], writtenFiles: [] };
  }

  const warnings = [];
  const writtenFiles = [];
  for (const file of response.files) {
    if (!isRecord(file)) continue;
    const name = typeof file.name === 'string' ? file.name.trim() : '';
    if (name.length === 0 || file.content === undefined || file.content === null) continue;

    const writePath = allowedResponseFilePath(name, parameterPaths);
    if (writePath === undefined) {
      warnings.push(`Skipped returned file outside the requested output path: ${name}`);
      continue;
    }

    const contents =
      file.encoding === 'base64'
        ? Buffer.from(String(file.content), 'base64')
        : Buffer.from(String(file.content), 'utf8');
    try {
      await atomicWritePrivate(writePath, contents);
      writtenFiles.push(writePath);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      warnings.push(`Failed to write returned file ${writePath}: ${message}`);
    }
  }
  return { warnings, writtenFiles };
}

function extractChannelText(value) {
  if (!isRecord(value)) return undefined;
  for (const channel of ['assistant', 'user']) {
    const items = value[channel];
    if (!Array.isArray(items)) continue;
    const text = items
      .filter((item) => isRecord(item) && item.type === 'text' && typeof item.text === 'string')
      .map((item) => item.text)
      .filter(Boolean)
      .join('\n\n')
      .trim();
    if (text.length > 0) return text;
  }
  return undefined;
}

function extractToolText(response) {
  if (typeof response === 'string') return response;
  if (!isRecord(response)) return String(response);
  if (response.is_success === false) {
    const detail = extractChannelText(response.error) ?? 'unknown tool error';
    throw new Error(`Tool API returned an error: ${detail}`);
  }
  return extractChannelText(response.result) ?? 'Tool API succeeded without a text result.';
}

const PYTHON_STRING_ESCAPES = {
  '\\': '\\',
  "'": "'",
  '"': '"',
  n: '\n',
  r: '\r',
  t: '\t',
  b: '\b',
  f: '\f',
};

function decodePythonString(value) {
  return value.replace(/\\(.)/gsu, (_, escaped) =>
    Object.hasOwn(PYTHON_STRING_ESCAPES, escaped)
      ? PYTHON_STRING_ESCAPES[escaped]
      : `\\${escaped}`,
  );
}

function extractArxivMarkdown(text) {
  let envelope;
  try {
    envelope = JSON.parse(text);
  } catch {
    return undefined;
  }
  if (!isRecord(envelope) || typeof envelope.is_success !== 'string') return undefined;

  const payload = envelope.is_success.trim();
  const match =
    /'content': '((?:\\.|[^'\\])*)'\}$/su.exec(payload) ??
    /'content': "((?:\\.|[^"\\])*)"\}$/su.exec(payload);
  return match === null ? undefined : decodePythonString(match[1]);
}

function appendTrace(text, trace) {
  const parts = [];
  if (trace.requestId !== undefined) parts.push(`request-id: ${trace.requestId}`);
  if (trace.refreshed === true) parts.push('token: refreshed');
  parts.push(`tool-call-id: ${trace.toolCallId}`);
  return `${text}\n\n[kimi-datasource] ${parts.join(' · ')}`;
}

export async function invokeTool(method, params) {
  let auth = await ensureFreshAccessToken();
  let attempt = await gatewayRequest(method, params, auth);
  let refreshed = auth.refreshed;

  if (attempt.response.status === 401) {
    auth = await ensureFreshAccessToken({ force: true });
    refreshed = true;
    attempt = await gatewayRequest(method, params, auth);
  }

  const storedToken = await loadTokenWire(credentialsPath(auth.kimiHome)).catch(() => undefined);
  const secrets = [...credentialSecrets(storedToken), auth.token];
  if (!attempt.response.ok) {
    const body = compactResponseBody(attempt.text, secrets) || 'empty response';
    throw new Error(`Kimi datasource HTTP ${attempt.response.status}: ${body}`);
  }

  let response;
  try {
    response = JSON.parse(attempt.text);
  } catch {
    response = attempt.text;
  }
  const { warnings, writtenFiles } = await writeResponseFiles(
    response,
    absoluteParameterPaths(params),
  );
  let text = extractToolText(response).trim();
  if (
    method === 'call_data_source_tool' &&
    params.data_source_name === 'arxiv' &&
    params.api_name === 'read_paper'
  ) {
    text = extractArxivMarkdown(text) ?? text;
  }
  if (warnings.length > 0) text = `${text}\n\n${warnings.join('\n')}`;
  text = redactSecrets(text, secrets);
  const trace = {
    requestId:
      attempt.requestId === undefined ? undefined : redactSecrets(attempt.requestId, secrets),
    toolCallId: attempt.toolCallId,
    refreshed,
  };
  return { text: appendTrace(text, trace), writtenFiles, trace };
}

export function parseArgv(argv) {
  const { values, positionals } = parseArgs({
    args: argv,
    options: CLI_OPTIONS,
    allowPositionals: true,
    strict: true,
  });
  return {
    command: positionals.shift() ?? 'help',
    positionals,
    flags: { ...values },
  };
}

function parseJsonObject(raw, source) {
  let parsed;
  try {
    parsed = JSON.parse(raw);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    throw new Error(`Invalid JSON from ${source}: ${message}`);
  }
  if (!isRecord(parsed)) throw new Error(`${source} must contain a JSON object.`);
  return parsed;
}

export async function buildCallParams(flags) {
  const jsonFile = flags['json-file'];
  const json = flags.json;
  if (typeof jsonFile === 'string' && typeof json === 'string') {
    throw new Error('Use either --json or --json-file, not both.');
  }
  if (typeof jsonFile === 'string') {
    const raw = await readFile(jsonFile === '-' ? 0 : jsonFile, 'utf8');
    return parseJsonObject(raw, jsonFile === '-' ? 'stdin' : `--json-file ${jsonFile}`);
  }
  return typeof json === 'string' ? parseJsonObject(json, '--json') : {};
}

function outputFormat(flags) {
  const format = flags.output ?? 'text';
  if (format !== 'text' && format !== 'json') {
    throw new Error(`Invalid --output value "${format}"; expected text or json.`);
  }
  return format;
}

function validateCommand(command, positionals, flags, count) {
  const allowed = COMMAND_FLAGS[command];
  for (const flag of Object.keys(flags)) {
    if (!allowed.has(flag)) throw new Error(`Option --${flag} is not valid for ${command}.`);
  }
  if (positionals.length > count) {
    throw new Error(`Unexpected argument for ${command}: ${positionals[count]}`);
  }
}

function stripTrace(text) {
  return text.replace(/\n\n\[kimi-datasource\][^\n]*\s*$/u, '').trimEnd();
}

function writeToolResult(result, flags, context) {
  if (outputFormat(flags) === 'text') {
    process.stdout.write(`${flags.quiet === true ? stripTrace(result.text) : result.text}\n`);
    return;
  }
  const output = {
    ok: true,
    ...context,
    text: stripTrace(result.text),
    files: result.writtenFiles,
  };
  if (flags.quiet !== true) output.trace = result.trace;
  process.stdout.write(`${JSON.stringify(output)}\n`);
}

function writeStatus(status, flags) {
  if (outputFormat(flags) === 'json') {
    process.stdout.write(`${JSON.stringify({ ok: status.kind === 'valid', ...status })}\n`);
    return;
  }
  process.stdout.write(`status: ${status.kind}\n`);
  process.stdout.write(`credential: ${status.credentialName}\n`);
  process.stdout.write(`path: ${status.filePath}\n`);
  if (status.expiresAt !== undefined) {
    process.stdout.write(`expires-at: ${new Date(status.expiresAt * 1000).toISOString()}\n`);
  }
  if (status.remainingSeconds !== undefined) {
    process.stdout.write(`remaining-seconds: ${status.remainingSeconds}\n`);
  }
  if (status.needsRefresh !== undefined) {
    process.stdout.write(`needs-refresh: ${String(status.needsRefresh)}\n`);
  }
  if (status.error !== undefined) process.stdout.write(`error: ${status.error}\n`);
}

export async function runCli(argv = process.argv.slice(2)) {
  assertSupportedNode();
  const { command, positionals, flags } = parseArgv(argv);
  if (flags.version === true) {
    process.stdout.write(`${VERSION}\n`);
    return 0;
  }
  if (command === 'help') {
    process.stdout.write(GLOBAL_HELP);
    return 0;
  }
  if (flags.help === true) {
    process.stdout.write(command === 'call' ? CALL_HELP : GLOBAL_HELP);
    return 0;
  }

  switch (command) {
    case 'status': {
      validateCommand(command, positionals, flags, 0);
      writeStatus(await credentialsStatus(), flags);
      return 0;
    }
    case 'refresh': {
      validateCommand(command, positionals, flags, 0);
      const result = await ensureFreshAccessToken({ force: true });
      if (outputFormat(flags) === 'json') {
        process.stdout.write(
          `${JSON.stringify({ ok: true, refreshed: result.refreshed, credential: credentialsPath() })}\n`,
        );
      } else if (flags.quiet !== true) {
        process.stdout.write(
          `${result.refreshed ? 'Kimi Code OAuth token refreshed.' : 'Kimi Code OAuth token is already current.'}\n`,
        );
      }
      return 0;
    }
    case 'sources': {
      validateCommand(command, positionals, flags, 0);
      if (outputFormat(flags) === 'json') {
        process.stdout.write(`${JSON.stringify({ ok: true, sources: DATA_SOURCE_NAMES })}\n`);
      } else {
        process.stdout.write(`${DATA_SOURCE_NAMES.join('\n')}\n`);
      }
      return 0;
    }
    case 'desc': {
      validateCommand(command, positionals, flags, 1);
      const source = positionals[0];
      if (source === undefined) throw new Error('Usage: kimi-datasource desc <source>');
      const result = await invokeTool('get_data_source_desc', { name: source });
      writeToolResult(result, flags, { command, source });
      return 0;
    }
    case 'call': {
      validateCommand(command, positionals, flags, 2);
      const [source, api] = positionals;
      if (source === undefined || api === undefined) {
        throw new Error('Usage: kimi-datasource call <source> <api> [options]');
      }
      const params = await buildCallParams(flags);
      const result = await invokeTool('call_data_source_tool', {
        data_source_name: source,
        api_name: api,
        params,
      });
      writeToolResult(result, flags, { command, source, api });
      return 0;
    }
    default:
      throw new Error(`Unknown command: ${command}\n\n${GLOBAL_HELP}`);
  }
}

async function currentSecrets() {
  const token = await loadTokenWire().catch(() => undefined);
  return credentialSecrets(token);
}

export async function runMain(argv = process.argv.slice(2)) {
  try {
    return await runCli(argv);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    process.stderr.write(`kimi-datasource: ${redactSecrets(message, await currentSecrets())}\n`);
    return 1;
  }
}

const entryPath = process.argv[1] === undefined ? undefined : path.resolve(process.argv[1]);
if (entryPath !== undefined && fileURLToPath(import.meta.url) === entryPath) {
  process.exitCode = await runMain();
}
