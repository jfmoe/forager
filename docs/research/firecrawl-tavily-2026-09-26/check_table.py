import json
from assess import ROOT, norm, source


def check_table():
    expected = []
    for figure in source('arxiv_html').select('figure'):
        caption = figure.select_one('figcaption')
        if caption and caption.get_text().startswith('Table 2:'):
            for row in figure.select('tr'):
                cells = row.find_all(['td', 'th'], recursive=False)
                if len(cells) == 14:
                    expected.append([norm(cell.get_text(' ', strip=True)) for cell in cells])
    if len(expected) != 7:
        raise ValueError('The source table does not have seven rows with fourteen cells.')
    results = {}
    for path in sorted(ROOT.glob('pdf_paper__firecrawl*.md')):
        lines = path.read_text().splitlines()
        actual = []
        for index, line in enumerate(lines):
            if line.startswith('| Model | Active Params | MMLU'):
                actual = [
                    [norm(cell.strip()) for cell in row.strip().strip('|').split('|')]
                    for row in lines[index:index + 8]
                    if row.startswith('|') and not row.startswith('| ---')
                ]
                break
        results[path.stem] = {
            'table2_rows': len(actual),
            'table2_columns': [len(row) for row in actual],
            'table2_exact_cells': sum(
                cell == reference
                for row, source_row in zip(actual, expected)
                for cell, reference in zip(row, source_row)
            ),
            'table2_expected_cells': 98,
        }
    return results


if __name__ == '__main__':
    print(json.dumps(check_table(), indent=2))
