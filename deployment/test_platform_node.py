import json
from pathlib import Path
import tempfile
import unittest
from platform_node import prepare


class FakeNode:
    indexed = True
    header = '00' * 164
    chain = 'main'
    ibd = False

    def __init__(self, url, cookie):
        self.cookie = Path(cookie)

    def call(self, method, params=()):
        if method == 'getblockchaininfo':
            return {'chain': self.chain, 'initialblockdownload': self.ibd, 'bestblockhash': 'test'}
        if method == 'getblockheader':
            return self.header
        if method == 'getindexinfo':
            return {'txindex': {'synced': self.indexed}}
        raise AssertionError(method)


class PlatformNodeTests(unittest.TestCase):
    env = {'PAPERCLIP_NODE_RPC_URL': 'http://node:18443', 'PAPERCLIP_NODE_RPC_USER': 'test', 'PAPERCLIP_NODE_RPC_PASSWORD': 'not-a-real-secret'}

    def test_indexed_node_and_credential_rotation(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            self.assertTrue(prepare(root, self.env, FakeNode))
            connection = json.loads((root / 'platform-wallet.json').read_text())
            self.assertNotIn('not-a-real-secret', json.dumps(connection))
            self.assertFalse((root / 'backend.json').exists())
            prepare(root, dict(self.env, PAPERCLIP_NODE_RPC_PASSWORD='rotated'), FakeNode)
            self.assertEqual((root / 'platform.cookie').read_text(), 'test:rotated')

    def test_pruned_node_preserves_adapter_auth_on_restart(self):
        class Pruned(FakeNode):
            indexed = False
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            prepare(root, self.env, Pruned)
            original = (root / 'client.cookie').read_bytes()
            prepare(root, self.env, Pruned)
            self.assertEqual(original, (root / 'client.cookie').read_bytes())
            self.assertEqual(json.loads((root / 'platform-wallet.json').read_text())['bitcoind']['bitcoind'], 'http://127.0.0.1:18336')

    def test_changed_provider_does_not_replace_saved_credentials(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            prepare(root, self.env, FakeNode)
            before = (root / 'platform.cookie').read_bytes()
            with self.assertRaises(ValueError):
                prepare(root, dict(self.env, PAPERCLIP_NODE_RPC_URL='http://different:8332', PAPERCLIP_NODE_RPC_PASSWORD='other'), FakeNode)
            self.assertEqual(before, (root / 'platform.cookie').read_bytes())

    def test_manual_adapter_is_not_overwritten(self):
        class Pruned(FakeNode):
            indexed = False
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / 'backend.json').write_text('{"rpc_url":"http://manual:8332"}')
            before = (root / 'backend.json').read_bytes()
            with self.assertRaises(ValueError):
                prepare(root, self.env, Pruned)
            self.assertEqual(before, (root / 'backend.json').read_bytes())
            self.assertFalse((root / 'platform-wallet.json').exists())

    def test_reject_wrong_chain_and_unsynced_node(self):
        for fields in ({'chain': 'regtest'}, {'header': '00' * 80}, {'ibd': True}):
            with tempfile.TemporaryDirectory() as temp:
                node = type('WrongNode', (FakeNode,), fields)
                with self.assertRaises(ValueError):
                    prepare(Path(temp), self.env, node)
                self.assertFalse((Path(temp) / 'platform-wallet.json').exists())

    def test_external_cookie_is_referenced_not_copied(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            cookie = root / 'external.cookie'
            cookie.write_text('external:secret')
            prepare(root, dict(self.env, PAPERCLIP_NODE_RPC_COOKIE=str(cookie)), FakeNode)
            self.assertFalse((root / 'platform.cookie').exists())
            self.assertIn(str(cookie), json.loads((root / 'platform-wallet.json').read_text())['bitcoind']['bitcoind_auth']['cookie']['cookie'])


if __name__ == '__main__':
    unittest.main()
