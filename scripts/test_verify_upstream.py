"""Negative controls: this quality gate must reject poisoned ancestry."""
from pathlib import Path
import importlib.util
import subprocess
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("verify_upstream", Path(__file__).with_name("verify_upstream.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class UpstreamProvenanceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        def git(*args):
            p = subprocess.run(["git", *args], cwd=self.root, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
            self.assertEqual(p.returncode, 0, p.stderr)
            return p.stdout.strip()
        self.git = git
        git("init", "-q")
        git("config", "user.email", "ci@example.invalid")
        git("config", "user.name", "CI")
        (self.root / "file").write_text("base")
        git("add", "file")
        git("commit", "-qm", "base")
        (self.root / "file").write_text("upstream")
        git("commit", "-qam", "upstream")
        self.upstream = git("rev-parse", "HEAD")
        git("branch", "provenance/master")
        (self.root / "file").write_text("private")
        git("commit", "-qam", "private")
        self.private = git("rev-parse", "HEAD")
        self.lock = self.root / "upstream-base.sha"
        self.lock.write_text(self.upstream + "\n")

    def check(self, **changes):
        return module.verify(str(self.root), str(self.lock), "provenance/master", **changes)

    def test_legitimate_upstream_ancestor_passes(self):
        self.assertEqual(self.check(), self.upstream)
        self.assertEqual(self.check(require_tip=True), self.upstream)

    def test_stale_upstream_lock_fails_strict_gate(self):
        self.git("branch", "-f", "provenance/master", self.private)
        self.assertEqual(self.check(), self.upstream)
        with self.assertRaises(ValueError):
            self.check(require_tip=True)

    def test_stale_lock_still_requires_real_upstream_lineage(self):
        self.git("branch", "-f", "provenance/master", self.private)
        self.lock.write_text("f" * 40 + "\n")
        with self.assertRaises(ValueError):
            self.check()

    def test_missing_object_fails(self):
        self.lock.write_text("f" * 40 + "\n")
        with self.assertRaises(ValueError):
            self.check()

    def test_fake_private_commit_cannot_claim_upstream_provenance(self):
        self.lock.write_text(self.private + "\n")
        with self.assertRaises(ValueError):
            self.check()

    def test_regressing_to_base_fails(self):
        with self.assertRaises(ValueError):
            self.check(head=self.git("rev-parse", "HEAD~2"))

    def test_malformed_lock_fails(self):
        self.lock.write_text(self.upstream[:9] + "\n")
        with self.assertRaises(ValueError):
            self.check()


if __name__ == "__main__":
    unittest.main()
