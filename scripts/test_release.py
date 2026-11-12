import tempfile
import unittest
from pathlib import Path
from release import update

class Version(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        (self.root/'benchmarks').mkdir()
        (self.root/'Cargo.toml').write_text('[package]\nname = "zsys"\nversion = "1.2.3"\n\n[features]\ndefault = []\n')
        lock = 'version = 4\n\n[[package]]\nname = "zsys"\nversion = "1.2.3"\n'
        for path in ('Cargo.lock','benchmarks/Cargo.lock'):
            (self.root/path).write_text(lock)
    def tearDown(self):
        self.temp.cleanup()
    def test_bumps_and_preserves_other_sections(self):
        for mode, expected in [('patch','1.2.4'),('minor','1.3.0'),('major','2.0.0'),('none','2.0.0')]:
            self.assertEqual(update(self.root,mode),expected)
            self.assertEqual(update(self.root,'none'),expected)
        self.assertIn('[features]\ndefault = []', (self.root/'Cargo.toml').read_text())
    def test_inconsistent_lock_does_not_modify_anything(self):
        before = (self.root/'Cargo.toml').read_text()
        (self.root/'benchmarks/Cargo.lock').write_text('[[package]]\nname = "zsys"\nversion = "9.0.0"\n')
        with self.assertRaises(ValueError): update(self.root,'patch')
        self.assertEqual((self.root/'Cargo.toml').read_text(),before)
    def test_invalid_mode_and_noop(self):
        before = (self.root/'Cargo.toml').read_bytes()
        with self.assertRaises(ValueError): update(self.root,'evil')
        self.assertEqual(update(self.root,'none'),'1.2.3')
        self.assertEqual((self.root/'Cargo.toml').read_bytes(),before)

if __name__ == '__main__': unittest.main()
