import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('packaging', Path(__file__).with_name('package.py'))
packaging = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packaging)


class PackageTest(unittest.TestCase):
    def test_manifest_security_and_backup_contracts(self):
        with tempfile.TemporaryDirectory() as directory:
            # Synthetic reference used only for generator tests, never published.
            image = 'example.invalid/paperclip:test@sha256:' + 'a' * 64
            for platform in ('umbrel', 'startos'):
                out = Path(directory) / platform
                packaging.package(platform, image, out)
                if platform == 'umbrel':
                    cfg = json.loads((out / 'docker-compose.yml').read_text())
                    service = cfg['services']['wallet']
                    self.assertNotIn('ports', service)
                    self.assertNotIn('PROXY_AUTH_ADD', cfg['services']['app_proxy']['environment'])
                    self.assertNotIn('APP_SEED', service['environment'])
                    self.assertEqual(service['user'], '1000:1000')
                else:
                    cfg = json.loads((out / 'manifest.yaml').read_text())
                    for action in ('create', 'restore'):
                        self.assertEqual(cfg['backup'][action]['mounts']['main'], '/data')
                        self.assertEqual(cfg['backup'][action]['args'][-1], '/data')
                    self.assertEqual(cfg['dependencies'], {}, 'Do not mandate a BTC node package')
                with self.assertRaises(FileExistsError):
                    packaging.package(platform, image, out)

    def test_unpinned_image_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaises(ValueError):
                packaging.package('umbrel', 'repo:latest', Path(directory) / 'new')


if __name__ == '__main__':
    unittest.main()
