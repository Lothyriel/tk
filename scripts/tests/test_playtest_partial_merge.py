import unittest

from scripts.playtest import partial_merge_evidence


class PartialMergeEvidenceTests(unittest.TestCase):
    preview = "GRID PARTIAL MERGE preview source=1 target=7 revision=2 source_quantity=2 target_quantity=2 remainder=1 total=3\n"
    confirmed = "GRID PARTIAL MERGE confirmed source=1 target=7 revision=3 source_quantity=1 target_quantity=3 op=1\n"

    def test_same_stacks_and_next_revision(self):
        report = partial_merge_evidence(self.preview + self.confirmed)
        self.assertEqual(report["after"], [1, 3])
        self.assertEqual(report["pending"], "not measured")

    def test_rejects_missing_duplicate_or_mismatched_observations(self):
        for log in (
            self.preview,
            self.confirmed + self.preview,
            self.preview + self.confirmed * 2,
            self.preview + self.confirmed.replace("target=7", "target=8"),
            self.preview + self.confirmed.replace("revision=3", "revision=4"),
        ):
            with self.subTest(log=log), self.assertRaises(ValueError):
                partial_merge_evidence(log)
