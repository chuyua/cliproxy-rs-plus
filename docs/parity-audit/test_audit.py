"""Run with: python3 -m unittest discover -s docs/parity-audit -p 'test_*.py'."""

import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("audit", Path(__file__).with_name("audit.py"))
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)
spec = importlib.util.spec_from_file_location("probe", Path(__file__).with_name("probe.py"))
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)
spec = importlib.util.spec_from_file_location("verify_reference", Path(__file__).with_name("verify_reference.py"))
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)


class SummaryTests(unittest.TestCase):
    def test_v8_035_routes_and_shared_config_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            Path(directory, "checklist.md").write_text(
                '- [ ] **[M1] M1-0128** `GET /v1/models/*model` — model detail\n'
                '- [ ] **[M5] M5-0304** `POST /v8/management/oauth/siwc/bind` — bind\n'
                '- [ ] **[M6] M6-0249** RPC `host.routing.reset_cooldown`\n'
                '- [ ] **[M6] M6-0250** Plugin data contract `ConfigField.Default`\n')
            with patch.object(probe, "HERE", directory):
                self.assertEqual(probe.routes(), [
                    ("M1-0128", "GET", "/v1/models/*model"),
                    ("M5-0304", "POST", "/v8/management/oauth/siwc/bind"),
                ])
            self.assertEqual(probe.concrete("/v1/models/*model"), "/v1/models/probe")
        for key, owner in (("upstream.claude.header-defaults.os", "claude"),
                           ("upstream.codex.response-steering", "codex"),
                           ("upstream.xai.inject-x-search", "openai-xai"),
                           ("oauth.providers.codex.base-url", "codex")):
            with self.subTest(key=key):
                self.assertEqual(audit.config_owner(key), owner)

    def test_plugin_http_fixtures_count_as_route_tests_but_not_callback_arguments(self):
        fixture = ('{"steps": [{"op": "http", "args": {"method": "DELETE", '
                   '"path": "/v8/management/plugins/example/quota"}}, '
                   '{"op": "respond", "args": {"method": "quota.reset"}}]}')
        with patch.object(audit, "scan_files", return_value=iter([
            ("crates/cpa-server/tests/fixtures/plugin_routes_go.json", fixture),
            ("crates/cpa-server/tests/fixtures/media_go.json",
             '{"scenarios": [{"method": "POST", "path": "/v1/videos/extensions"}]}'),
        ])):
            tests = audit.test_texts()
        key = "DELETE /v8/management/plugins/:id/quota"
        result = audit.judge_route({"text": f"`{key}`"}, {key: {"routed": True, "status": 400}}, tests)
        self.assertEqual(result[0], "covered")
        for key in ("GET /v8/management/plugins/:id/quota", "DELETE /v8/management/plugins/:id/config"):
            result = audit.judge_route({"text": f"`{key}`"}, {key: {"routed": True, "status": 400}}, tests)
            self.assertEqual(result[0], "partial")
        key = "POST /v1/videos/extensions"
        result = audit.judge_route({"text": f"`{key}`"}, {key: {"routed": True, "status": 401}}, tests)
        self.assertEqual(result[0], "covered")
        self.assertNotIn("quota.reset", "\n".join(tests.values()))

    def test_fixture_route_parameters_stop_at_query_quote_and_line_boundaries(self):
        quota = "DELETE /v8/management/plugins/:id/quota"
        models = "GET /v1/models/*model"
        cases = [
            (quota, 'DELETE "/v8/management/plugins/example/quota?next=/other"', "covered"),
            (quota, 'DELETE "/v8/management/plugins/example?next=/quota"', "partial"),
            (models, 'GET "/v1/models/provider/model?next=/other"', "covered"),
            (models, 'GET "/v1/models/?next=/provider/model"', "partial"),
            (models, 'GET "/v1/models/provider" "unrelated"', "partial"),
            (quota, 'DELETE "/v8/management/plugins/ex\rample/quota"', "partial"),
            (quota, 'DELETE "/v8/management/plugins/ex\nample/quota"', "partial"),
            (models, 'GET "/v1/models/provider\rmodel"', "partial"),
            (models, 'GET "/v1/models/provider\nmodel"', "partial"),
        ]
        for key, request, expected in cases:
            with self.subTest(key=key, request=request):
                result = audit.judge_route(
                    {"text": f"`{key}`"}, {key: {"routed": True, "status": 400}},
                    {"requests.json": request})
                self.assertEqual(result[0], expected)

    def test_both_public_summaries_use_the_same_counts_and_are_idempotent(self):
        rows = [
            {"ms": "M1", "status": "covered"},
            {"ms": "M1", "status": "partial"},
            {"ms": "M1", "status": "partial"},
            {"ms": "M2", "status": "missing"},
        ]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "docs").mkdir()
            readme = root / "README.md"
            parity = root / "docs/PARITY.md"
            readme.write_text("Before. Parity is incomplete: the audit has 99 items, with "
                              "9 covered, 80 partial and 10 missing. After.\n")
            parity.write_text("Before\n<!-- parity-summary:start -->\nstale\n"
                              "<!-- parity-summary:end -->\nAfter\n")
            with patch.object(audit, "ROOT", str(root)):
                audit.summary(rows)
                first = (readme.read_text(), parity.read_text())
                self.assertEqual(first[0], "Before. Parity is incomplete: the audit has 4 items, "
                                 "with 1 covered, 2 partial and 1 missing. After.\n")
                self.assertIn("| M1 | 3 | 1 | 2 | 0 |", first[1])
                self.assertIn("| M2 | 1 | 0 | 0 | 1 |", first[1])
                self.assertIn("| All | 4 | 1 | 2 | 1 |", first[1])
                audit.summary(rows)
                self.assertEqual(first, (readme.read_text(), parity.read_text()))

    def test_invalid_readme_summary_leaves_all_pages_unchanged(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "docs").mkdir()
            paths = [root / "README.md", root / "docs/PARITY.md", root / "status.md"]
            paths[1].write_text("<!-- parity-summary:start -->\nstale\n<!-- parity-summary:end -->")
            paths[2].write_text("Existing status page")
            sentence = "Parity is incomplete: the audit has 9 items, with 3 covered, 4 partial and 2 missing."
            for readme in ("Summary was reworded", sentence + "\n" + sentence):
                with self.subTest(readme=readme):
                    paths[0].write_text(readme)
                    before = [p.read_text() for p in paths]
                    with patch.object(audit, "ROOT", str(root)), patch.object(audit, "HERE", str(root)):
                        with self.assertRaisesRegex(ValueError, "exactly one parity summary"):
                            audit.render([])
                    self.assertEqual(before, [p.read_text() for p in paths])


class RouteRegistrationTests(unittest.TestCase):
    def test_full_paths_resolve_groups_constants_loops_and_muxes(self):
        source = """
func (s *Server) routes() {
	const prefix = "/v8/management"
	s.engine.GET(prefix+"/oauth/callback", h)
	v1 := s.engine.Group("/v1")
	v1.POST("/audio/speech", h)
	nested := v1.Group("/models")
	nested.Any("/*model", h)
	v8 := s.engine.Group(prefix)
	for _, method := range []string{http.MethodGet, http.MethodDelete} {
		v8.Handle(method, "/config/*path", h)
	}
	v8.Handle("PUT", "/config.yaml", h)
	mux := http.NewServeMux()
	mux.HandleFunc("/auth/callback", h)
	_, _ = windows.LocalFree(windows.Handle(ptr))
}
"""
        self.assertEqual(verify.registrations_in("x.go", source), [
            (4, "GET", "/v8/management/oauth/callback"),
            (6, "POST", "/v1/audio/speech"),
            (8, "ANY", "/v1/models/*model"),
            (11, "GET", "/v8/management/config/*path"),
            (11, "DELETE", "/v8/management/config/*path"),
            (13, "PUT", "/v8/management/config.yaml"),
            (15, "ANY", "/auth/callback"),
        ])

    def test_unresolved_registrations_fail(self):
        for line in ('router.GET("/v1/x", h)', "s.engine.POST(path, h)", 'g := s.engine.Group(base)'):
            with self.subTest(line=line), self.assertRaises(SystemExit):
                verify.registrations_in("x.go", line)


class SuiteSkeletonTests(unittest.TestCase):
    ROW = ("**a/x_test.go:1-90** — golden/behavior cases: `TestA` (L3). Removed upstream declarations: "
           "`TestB`, `TestC` (historical, not active requirements). Moved upstream declarations: "
           "`TestD` to a/y_test.go:4; `TestE` to a/y_test.go:9.")

    def test_regenerated_fragments_are_ignored(self):
        regenerated = (self.ROW.replace("1-90", "1-95").replace("(L3)", "(L5)").replace("`TestC`", "`TestF`")
                       .replace("y_test.go:9", "z_test.go:12"))
        self.assertEqual(verify.suite_skeleton(regenerated), verify.suite_skeleton(self.ROW))

    def test_every_stripped_fragment_is_returned_for_validation(self):
        extra = self.ROW + (" Removed upstream declarations: `TestG` (historical, not active requirements)."
                            " Moved upstream declarations: `TestH` to a/w_test.go:7.")
        _, _, retired, moved = verify.suite_parts(extra)
        self.assertEqual(retired, ["TestB", "TestC", "TestG"])
        self.assertEqual(moved, [("TestD", "a/y_test.go", 4), ("TestE", "a/y_test.go", 9), ("TestH", "a/w_test.go", 7)])

    def test_a_moved_entry_without_a_target_line_stays_compared(self):
        bogus = self.ROW + " Moved upstream declarations: `TestBogus` to missing_test.go."
        self.assertNotEqual(verify.suite_skeleton(bogus), verify.suite_skeleton(self.ROW))
        self.assertNotIn("TestBogus", [name for name, _, _ in verify.suite_parts(bogus)[3]])

    def test_removing_one_fragment_cannot_form_another_that_goes_unchecked(self):
        nested = self.ROW.replace(
            " Removed upstream declarations:",
            " Moved upstream declarations: `TestBogus` to missing_test.go:3 Removed upstream declarations:")
        nested = nested.replace("requirements).", "requirements)..")
        stripped, _, _, moved = verify.suite_parts(nested)
        self.assertIn("TestBogus", stripped)
        self.assertNotIn("TestBogus", [name for name, _, _ in moved])
        self.assertNotEqual(verify.suite_skeleton(nested), verify.suite_skeleton(self.ROW))

    def test_removing_history_cannot_form_a_declaration_that_goes_unchecked(self):
        forged = self.ROW.replace(
            "(L3).",
            "(L3); `TestBogus` (L999 Removed upstream declarations: `TestZ` (historical, not active requirements).).")
        skeleton, declared, retired, _ = verify.suite_parts(forged)
        self.assertEqual(declared, [("TestA", 3)])
        self.assertIn("TestZ", retired)
        self.assertTrue(verify.MARKER.search(skeleton))
        self.assertNotEqual(skeleton, verify.suite_skeleton(self.ROW))

    def test_removing_history_cannot_form_a_link_that_hides_text(self):
        one = " Removed upstream declarations: `{}` (historical, not active requirements)."
        forged = self.ROW.replace(
            " Removed upstream declarations: `TestB`, `TestC` (historical, not active requirements).",
            "[`" + one.format("TestB") + "`](`TestBogus` (L999" + one.format("TestC") + ")")
        skeleton, declared, retired, _ = verify.suite_parts(forged)
        self.assertEqual(retired, ["TestB", "TestC"])
        self.assertEqual(declared, [("TestA", 3)])
        self.assertIn("TestBogus", skeleton)
        self.assertNotEqual(skeleton, verify.suite_skeleton(self.ROW))

    def test_the_sentinel_is_rejected_in_input(self):
        with self.assertRaises(SystemExit):
            verify.suite_parts(self.ROW + verify.SENTINEL)

    def test_a_clean_skeleton_keeps_no_markers(self):
        self.assertIsNone(verify.MARKER.search(verify.suite_skeleton(self.ROW)))

    def test_prose_around_or_inside_history_stays_compared(self):
        for changed in (self.ROW + " All cases are optional.",
                        self.ROW.replace("(L3).", "(L3). Optional."),
                        self.ROW.replace("`TestB`, `TestC`", "`TestB`, optional, `TestC`"),
                        self.ROW.replace("y_test.go:4; ", "y_test.go:4; optional; ")):
            with self.subTest(changed=changed):
                self.assertNotEqual(verify.suite_skeleton(changed), verify.suite_skeleton(self.ROW))


if __name__ == "__main__":
    unittest.main()
