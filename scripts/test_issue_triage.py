import copy
import os
import tempfile
from pathlib import Path
import unittest
from unittest.mock import patch

import issue_triage as bot


class TriageTests(unittest.TestCase):
    def setUp(self):
        self.issue = {"number": 1, "title": "Exportar MP4 falla", "body": "Se bloquea exportando audio", "state": "open", "labels": []}
        self.candidates = [{"number": 2, "title": "Exportación", "body": "", "state": "closed"}]
        self.result = {"priority": "high", "confidence": 0.9, "reason": "La exportación está bloqueada.",
                       "related": [{"number": 2, "reason": "Afecta al mismo flujo."}]}

    def test_rejects_invented_related_issue(self):
        self.result["related"][0]["number"] = 999
        with self.assertRaises(ValueError):
            bot.validate_result(self.result, self.candidates)

    def test_rejects_duplicate_references(self):
        self.result["related"] *= 2
        with self.assertRaises(ValueError):
            bot.validate_result(self.result, self.candidates)

    def test_rejects_invalid_confidence(self):
        for value in [float("nan"), float("inf"), True, -1, 2, "high"]:
            with self.subTest(value=value):
                self.result["confidence"] = value
                with self.assertRaises(ValueError):
                    bot.validate_result(self.result, self.candidates)

    def test_preserves_existing_priority_and_other_labels(self):
        self.issue["labels"] = [{"name": "bug"}, {"name": "priority: low"}]
        before = copy.deepcopy(self.issue)
        self.assertIsNone(bot.label_plan(self.issue, self.result, 0.75))
        self.assertEqual(self.issue, before)

    def test_manual_opt_out(self):
        self.issue["labels"] = [{"name": "triage:manual"}]
        self.assertIsNone(bot.label_plan(self.issue, self.result, 0.75))

    def test_low_confidence_requires_review(self):
        self.result["confidence"] = 0.4
        self.assertEqual(bot.label_plan(self.issue, self.result, 0.75), "triage: needs-review")

    def test_high_confidence_sets_priority(self):
        self.assertEqual(bot.label_plan(self.issue, self.result, 0.75), "priority: high")

    def test_filters_self_and_pull_requests(self):
        issues = [self.issue, {**self.issue, "number": 3, "pull_request": {}},
                  {**self.issue, "number": 4}]
        self.assertEqual([x["number"] for x in bot.candidates_for(self.issue, issues)], [4])

    def test_caps_candidates_and_text(self):
        issues = [{**self.issue, "number": i, "body": "x" * 10000} for i in range(2, 100)]
        candidates = bot.candidates_for(self.issue, issues)
        self.assertEqual(len(candidates), 20)
        self.assertTrue(all(len(x["body"]) <= 2500 for x in candidates))

    def test_sanitizes_model_mentions_links_and_markup(self):
        result = bot.safe_text("@everyone <script> [click](https://evil.example) #999")
        self.assertNotIn("@", result)
        self.assertNotIn("<script>", result)
        self.assertNotIn("https://", result)
        self.assertNotIn("#999", result)

    def test_closed_issue_is_reported_as_closed(self):
        body = bot.comment_body(self.result, self.candidates, "priority: high", self.issue)
        self.assertIn("#2 (cerrado)", body)

    def test_empty_candidates_do_not_invent_matches(self):
        self.result["related"] = []
        bot.validate_result(self.result, [])
        self.assertIn("No se encontraron", bot.comment_body(self.result, [], None, self.issue))

    def test_comment_updates_only_owned_bot_comment(self):
        class FakeGitHub:
            def __init__(self):
                self.writes = []

            def pages(self, path):
                return [
                    {"id": 1, "body": bot.MARKER, "user": {"login": "someone", "type": "User"}},
                    {"id": 2, "body": bot.MARKER, "user": {"login": "github-actions[bot]", "type": "Bot"}},
                ]

            def call(self, *args):
                self.writes.append(args)

        gh = FakeGitHub()
        bot.update_comment(gh, 5, "new body", "github-actions[bot]")
        self.assertEqual(gh.writes, [("/issues/comments/2", "PATCH", {"body": "new body"})])

    def test_gemini_request_and_completed_response(self):
        config = {"project_context": "video editor", "priorities": {"high": "blocked"}}
        with patch.object(bot, "request", return_value={"status": "completed", "output_text": bot.json.dumps(self.result)}) as request:
            self.assertEqual(bot.classify(self.issue, self.candidates, config, "test-key", "model"), self.result)
        payload = request.call_args.args[3]
        self.assertFalse(payload["store"])
        self.assertNotIn("tools", payload)
        self.assertNotIn("test-key", bot.json.dumps(payload))

    def test_incomplete_gemini_response_is_not_published(self):
        with patch.object(bot, "request", return_value={"status": "in_progress"}):
            with self.assertRaises(ValueError):
                bot.classify(self.issue, [], {"project_context": "", "priorities": {}}, "key", "model")

    def test_rest_model_output_steps(self):
        response = {"status": "completed", "steps": [{"type": "model_output", "content": [
            {"type": "text", "text": bot.json.dumps(self.result)}]}]}
        with patch.object(bot, "request", return_value=response):
            self.assertEqual(bot.classify(self.issue, self.candidates,
                                         {"project_context": "", "priorities": {}}, "key", "model"), self.result)


    def test_dry_run_never_writes_to_github(self):
        class FakeGitHub:
            def __init__(self, issue):
                self.issue = issue
                self.methods = []
            def call(self, path, method="GET", data=None):
                self.methods.append(method)
                return self.issue
            def pages(self, path):
                return []
        gh = FakeGitHub(self.issue)
        result = {**self.result, "related": []}
        with tempfile.TemporaryDirectory() as folder:
            event = Path(folder) / "event.json"
            event.write_text("{}", encoding="utf-8")
            summary = Path(folder) / "summary.md"
            env = {"GITHUB_EVENT_PATH": str(event), "GITHUB_REPOSITORY": "owner/repo",
                   "GH_TOKEN": "test", "GEMINI_API_KEY": "test", "ISSUE_NUMBER": "1",
                   "DRY_RUN": "true", "GITHUB_STEP_SUMMARY": str(summary)}
            with patch.dict(os.environ, env), patch.object(bot, "GitHub", return_value=gh), \
                    patch.object(bot, "classify", return_value=result):
                bot.main()
            self.assertTrue(summary.exists())
            self.assertTrue(all(method == "GET" for method in gh.methods))

    def test_changed_issue_discards_result(self):
        from unittest.mock import MagicMock
        gh = MagicMock()
        gh.call.side_effect = [self.issue, {**self.issue, "body": "Changed during inference"}]
        gh.pages.return_value = []
        with tempfile.TemporaryDirectory() as folder:
            event = Path(folder) / "event.json"
            event.write_text("{}", encoding="utf-8")
            env = {"GITHUB_EVENT_PATH": str(event), "GITHUB_REPOSITORY": "owner/repo",
                   "GH_TOKEN": "test", "GEMINI_API_KEY": "test", "ISSUE_NUMBER": "1", "DRY_RUN": "false"}
            with patch.dict(os.environ, env), patch.object(bot, "GitHub", return_value=gh), \
                    patch.object(bot, "classify", return_value={**self.result, "related": []}):
                bot.main()
            self.assertEqual(gh.call.call_count, 2)


if __name__ == "__main__":
    unittest.main()
