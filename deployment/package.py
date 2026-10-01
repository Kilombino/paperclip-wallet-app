#!/usr/bin/env python3
"""Materialize Paperclip beta platform candidates from an explicitly pinned, tested image."""
import argparse
import json
from pathlib import Path
import re
import shutil


def write(path, value):
    path.write_text(json.dumps(value, indent=2) + '\n', encoding='utf-8')


def package(platform, image, destination, app='paperclip-wallet'):
    if not re.fullmatch(r'[a-z0-9][a-z0-9.:/_-]*:[A-Za-z0-9_.-]+@sha256:[0-9a-f]{64}', image):
        raise ValueError('Supply a tag and immutable multi-architecture image index digest')
    if not re.fullmatch(r'[a-z][a-z0-9]*(-[a-z0-9]+)+', app):
        raise ValueError('Invalid app ID')
    if platform == 'startos' and app != 'paperclip-wallet':
        raise ValueError('Custom app IDs currently apply to Umbrel only')
    root = Path(__file__).resolve().parents[1]
    out = Path(destination)
    out.mkdir(parents=True, exist_ok=False)
    if platform == 'umbrel':
        (out / 'hooks').mkdir()
        shutil.copyfile(root / 'deployment/umbrel-pre-start', out / 'hooks/pre-start')
        (out / 'hooks/pre-start').chmod(0o755)
        write(out / 'umbrel-app.yml', {
            'manifestVersion': 1, 'id': app, 'name': 'Paperclip Wallet Beta', 'tagline': 'On-chain, Ark, and Lightning for XBT',
            'category': 'bitcoin', 'version': '0.7.4', 'port': 38180,
            'description': 'Beta XBT wallet preset to https://ark.paperclippool.xyz. Configure your compatible XBT blockchain backend. Wait for server activation before funding. Back up the complete wallet.',
            'developer': 'Paperclip', 'website': 'https://github.com/connorslab/paperclip-wallet-app',
            'repo': 'https://github.com/connorslab/paperclip-wallet-app',
            'support': 'https://github.com/connorslab/paperclip-wallet-app/issues',
            'icon': 'https://raw.githubusercontent.com/connorslab/paperclip-wallet-app/main/web/icon.svg', 'dependencies': [], 'gallery': [], 'path': '', 'defaultUsername': '',
            'deterministicPassword': True, 'submitter': 'Paperclip',
            'releaseNotes': 'Paperclip beta server preset. Local keys, authenticated access, on-chain, Ark, and server-backed Lightning. Platform restore acceptance testing remains required.'
        })
        write(out / 'docker-compose.yml', {'services': {
            'app_proxy': {'environment': {'APP_HOST': app + '_wallet_1', 'APP_PORT': '3000'}},
            'wallet': {'image': image, 'user': '1000:1000', 'restart': 'unless-stopped',
                'stop_grace_period': '2m', 'security_opt': ['no-new-privileges:true'],
                'cap_drop': ['ALL'], 'volumes': ['${APP_DATA_DIR}/data:/data'],
                'environment': {'APP_PASSWORD': '${APP_PASSWORD}', 'PAPERCLIP_XBT_MAINNET': '1', 'BARKD_UI_DEFAULT_ARK_SERVER': 'https://ark.paperclippool.xyz'}}
        }})
        (out / 'data').mkdir()
        (out / 'data/.gitkeep').touch()
    elif platform == 'startos':
        manifest = {
            'id': app, 'title': 'Paperclip Wallet Beta', 'version': '0.7.4.0', 'license': 'MIT',
            'release-notes': 'Beta package for StartOS 0.3.5 only. Paperclip server preset. Not compatible with StartOS 0.4.',
            'wrapper-repo': 'https://github.com/connorslab/paperclip-wallet-app',
            'upstream-repo': 'https://github.com/connorslab/paperclip-wallet-app',
            'support-site': 'https://github.com/connorslab/paperclip-wallet-app/issues',
            'marketing-site': 'https://github.com/connorslab/paperclip-wallet-app', 'build': ['make'],
            'description': {'short': 'Your XBT Ark wallet', 'long': 'Beta XBT wallet preset to Paperclip. Configure your XBT blockchain backend. Back up the complete wallet before funding.'},
            'assets': {'license': 'LICENSE', 'icon': 'icon.svg', 'instructions': 'INSTRUCTIONS.md'},
            'main': {'type': 'docker', 'image': 'main', 'entrypoint': '/usr/bin/tini',
                'args': ['--', 'python3', '/usr/local/lib/paperclip/startos-entrypoint.py'],
                'mounts': {'main': '/data'}, 'gpu-acceleration': False, 'sigterm-timeout': '2m'},
            'hardware-requirements': {'arch': ['x86_64', 'aarch64']},
            'config': None, 'properties': None, 'dependencies': {},
            'health-checks': {'api': {'name': 'Wallet API',
                'success-message': 'Authenticated wallet API is available; this does not certify backend sync or funds.',
                'type': 'docker', 'image': 'main', 'system': False, 'entrypoint': 'python3',
                'args': ['/usr/local/lib/paperclip/health.py'], 'mounts': {}, 'inject': True, 'io-format': 'json'}},
            'volumes': {'main': {'type': 'data'}},
            'interfaces': {'main': {'name': 'Wallet', 'description': 'Authenticated wallet and setup',
                'tor-config': {'port-mapping': {'80': '3000'}},
                'lan-config': {'443': {'ssl': True, 'internal': 3000}},
                'ui': True, 'protocols': ['tcp', 'http']}},
            'actions': {'access-token': {'name': 'Show wallet access token',
                'description': 'Retrieve the private token required to unlock the wallet UI.',
                'warning': 'This token grants spending access. Keep it private.',
                'allowed-statuses': ['running', 'stopped'],
                'implementation': {'type': 'docker', 'image': 'main', 'system': False,
                    'entrypoint': 'python3', 'args': ['/usr/local/lib/paperclip/startos-action.py'],
                    'mounts': {'main': '/data'}, 'io-format': 'json'}}},
            'backup': {}, 'migrations': {'from': {}, 'to': {}}
        }
        for action in ('create', 'restore'):
            manifest['backup'][action] = {'type': 'docker', 'image': 'compat', 'system': True,
                'entrypoint': 'compat', 'args': ['duplicity', action, '/mnt/backup', '/data'],
                'mounts': {'BACKUP': '/mnt/backup', 'main': '/data'}}
        write(out / 'manifest.yaml', manifest)
        (out / 'Dockerfile').write_text(f'FROM {image}\nUSER root\n', encoding='utf-8')
        (out / 'Makefile').write_text('''all: verify

docker-images/x86_64.tar: Dockerfile
	mkdir -p docker-images
	docker buildx build --platform linux/amd64 --tag start9/paperclip-wallet/main:0.7.1.0 --output type=docker,dest=$@ .

docker-images/aarch64.tar: Dockerfile
	mkdir -p docker-images
	docker buildx build --platform linux/arm64 --tag start9/paperclip-wallet/main:0.7.1.0 --output type=docker,dest=$@ .

paperclip-wallet.s9pk: manifest.yaml Dockerfile LICENSE icon.svg INSTRUCTIONS.md docker-images/x86_64.tar docker-images/aarch64.tar
	start-sdk pack

verify: paperclip-wallet.s9pk
	start-sdk verify s9pk paperclip-wallet.s9pk
''', encoding='utf-8')
        shutil.copyfile(root / 'LICENSE', out / 'LICENSE')
        (out / 'icon.svg').write_text('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128"><rect width="128" height="128" rx="24" fill="#111c2e"/><text x="64" y="92" text-anchor="middle" font-family="sans-serif" font-weight="bold" font-size="88" fill="#f56835">P</text></svg>\n', encoding='utf-8')
    else:
        raise ValueError('Unknown platform')
    shutil.copyfile(root / 'web/icon.svg', out / 'icon.svg')
    shutil.copyfile(root / 'deployment/BUNDLED-PRUNED.md', out / 'BUNDLED-PRUNED.md')
    shutil.copyfile(root / 'deployment/PRUNED-NODES.md', out / 'PRUNED-NODES.md')
    shutil.copyfile(root / 'deployment/PLATFORMS.md', out / 'INSTRUCTIONS.md')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('platform', choices=['umbrel', 'startos'])
    parser.add_argument('--image', required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--app-id', default='paperclip-wallet')
    args = parser.parse_args()
    package(args.platform, args.image, args.output, args.app_id)
