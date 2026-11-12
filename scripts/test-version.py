#!/usr/bin/env python3
"""Regression checks for release numbering, including carry/reset and rejected inputs."""
import unittest
from version import bumped, update_lock


class Versions(unittest.TestCase):
    def test_semver_bumps(self):
        self.assertEqual(bumped("1.9.9", "patch"), "1.9.10")
        self.assertEqual(bumped("1.9.9", "minor"), "1.10.0")
        self.assertEqual(bumped("1.9.9", "major"), "2.0.0")
        self.assertEqual(bumped("0.1.0", "none"), "0.1.0")

    def test_rejects_non_release_versions(self):
        for value in ("01.0.0", "1.2", "1.2.3-rc.1", "1.2.3\n", "v1.2.3"):
            with self.assertRaises(ValueError):
                bumped(value, "patch")
        with self.assertRaises(ValueError):
            bumped("1.2.3", "arbitrary")

    def test_lock_update_preserves_unrelated_packages(self):
        text = 'version = 4\n\n[[package]]\nname = "sysinfo"\nversion = "0.40.3"\n\n[[package]]\nname = "zsys"\nversion = "0.1.0"\n'
        changed = update_lock(text, "0.1.0", "0.2.0")
        self.assertEqual(changed, text.replace('version = "0.1.0"', 'version = "0.2.0"'))
        with self.assertRaises(ValueError):
            update_lock(text, "0.0.9", "0.2.0")


if __name__ == "__main__":
    unittest.main()
