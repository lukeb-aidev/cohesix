# CLASSIFICATION: COMMUNITY
# Filename: scan.py v0.1
# Author: Lukas Bower
# Date Modified: 2025-07-12
"""Repository OSS dependency scanner for Cohesix."""

from __future__ import annotations
import argparse
import hashlib
import json
import os
import urllib.request
from pathlib import Path
import tomli

from .license_fetch import fetch_license_text
from .security_check import query_osv


def parse_cargo_toml(path: Path) -> list[tuple[str, str, str]]:
    """Return list of (name, version, source_url) for dependencies."""
    deps = []
    data = tomli.loads(path.read_text())
    for section in ("dependencies", "build-dependencies", "dev-dependencies"):
        table = data.get(section, {})
        for name, spec in table.items():
            if isinstance(spec, str):
                version = spec
            elif isinstance(spec, dict):
                version = spec.get("version", "*")
                if "path" in spec:
                    continue
            else:
                continue
            deps.append((name, version, f"https://crates.io/crates/{name}"))
    return deps


def scan_paths(paths: list[str]) -> list[dict]:
    deps: list[dict] = []
    for root in paths:
        for cargo_file in Path(root).rglob('Cargo.toml'):
            for name, version, url in parse_cargo_toml(cargo_file):
                deps.append({'ecosystem': 'crates.io', 'name': name, 'version': version, 'source': url})
    return deps


def generate_outputs(deps: list[dict], outdir: Path) -> None:
    outdir.mkdir(parents=True, exist_ok=True)
    license_dir = outdir / 'LICENSES'
    license_dir.mkdir(exist_ok=True)

    sbom_spdx = {
        'SPDXID': 'SPDXRef-DOCUMENT',
        'spdxVersion': 'SPDX-2.3',
        'name': 'Cohesix OSS SBOM',
        'packages': []
    }
    sbom_cdx = {
        'bomFormat': 'CycloneDX',
        'specVersion': '1.5',
        'version': 1,
        'components': []
    }

    md_lines = ['// CLASSIFICATION: COMMUNITY', '# Open Source Dependencies', '', '| Name | Version | License |', '|------|---------|---------|']
    matrix_lines = ['// CLASSIFICATION: COMMUNITY', '# License Matrix', '', '| Name | Version | SPDX | License File | CVEs |', '|------|---------|------|-------------|------|']

    for dep in deps:
        name = dep['name']
        version = dep['version']
        url = dep['source']
        meta_data = json.loads(
            urllib.request.urlopen(f'https://crates.io/api/v1/crates/{name}').read().decode()
        )
        license_id = meta_data['crate'].get('license')
        if not license_id:
            versions = meta_data.get('versions') or []
            if versions:
                license_id = versions[0].get('license')
        license_id = license_id or 'UNKNOWN'
        lic_text = fetch_license_text(name, version, license_id, url)
        lic_file = license_dir / f"{name}-{version}.txt"
        lic_file.write_text(lic_text)
        sha = hashlib.sha256(lic_text.encode()).hexdigest()
        vulns = query_osv('crates.io', name, version)
        cve_summary = ', '.join(v['id'] for v in vulns) if vulns else ''

        sbom_spdx['packages'].append({
            'name': name,
            'SPDXID': f'SPDXRef-{name}',
            'versionInfo': version,
            'downloadLocation': url,
            'licenseConcluded': license_id,
            'checksums': [{'alg': 'SHA256', 'checksumValue': sha}],
        })
        sbom_cdx['components'].append({
            'type': 'library',
            'name': name,
            'version': version,
            'licenses': [{'license': {'id': license_id}}],
            'hashes': [{'alg': 'SHA-256', 'content': sha}],
        })
        md_lines.append(f'| {name} | {version} | {license_id} |')
        matrix_lines.append(
            f'| {name} | {version} | {license_id} | LICENSES/{lic_file.name} | {cve_summary} |'
        )

    (outdir / 'OPEN_SOURCE_DEPENDENCIES.md').write_text('\n'.join(md_lines))
    (outdir / 'LICENSE_MATRIX.md').write_text('\n'.join(matrix_lines))
    (outdir / 'sbom_spdx_2.3.json').write_text(json.dumps(sbom_spdx, indent=2))
    (outdir / 'sbom_cyclonedx_1.5.json').write_text(json.dumps(sbom_cdx, indent=2))


def run_audit(paths: list[str], outdir: str) -> None:
    deps = scan_paths(paths)
    generate_outputs(deps, Path(outdir))


def main():
    parser = argparse.ArgumentParser(description='Cohesix OSS dependency audit')
    parser.add_argument('paths', nargs='*', default=['.'])
    parser.add_argument('--output', default='docs/community')
    parser.add_argument('--demo', action='store_true', help='generate small demo output')
    args = parser.parse_args()
    if args.demo:
        deps = scan_paths(['.'])[:2]
        generate_outputs(deps, Path(args.output))
    else:
        run_audit(args.paths, args.output)


if __name__ == '__main__':
    main()
