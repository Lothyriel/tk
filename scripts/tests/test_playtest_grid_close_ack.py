import unittest

from scripts.playtest import grid_close_ack_evidence


def trace(*events, outcome):
    rows = (("submitted", 5), ("pending_presented", 5), *events, ("reopened", 8), (outcome, 8))
    return "\n".join(f"INFO GRID CLOSE ACK {stage} raid=1 session=2 op=3 item=4 revision=5 frame={frame}"
                     for stage, frame in rows)


class GridCloseAckTests(unittest.TestCase):
    def test_close_first_requires_both_unchanged_revision_and_no_ack_at_tab(self):
        log = trace(("tab_close", 6), ("delta_applied", 7), ("ack_applied", 7),
                    outcome="CLOSE_BEFORE_ACK")
        self.assertEqual(grid_close_ack_evidence(log)["operation"], 3)
        for bad in (log.replace("tab_close raid=1", "tab_close raid=2"),
                    log.replace("revision=5 frame=6", "revision=5 frame=5"),
                    log.replace("ack_applied", "duplicate_submit"),
                    log + "\n" + log.splitlines()[0]):
            with self.assertRaises(ValueError):
                grid_close_ack_evidence(bad)

    def test_same_frame_receive_before_tab_is_not_observed(self):
        for before, after, outcome in (
            ("ack_applied", "delta_applied", "ACK_FIRST_NOT_OBSERVED"),
            ("delta_applied", "ack_applied", "DELTA_FIRST_NOT_OBSERVED"),
        ):
            log = trace((before, 6), ("tab_close", 6), (after, 7), outcome=outcome)
            self.assertEqual(grid_close_ack_evidence(log)["status"], outcome)
            with self.assertRaises(ValueError):
                grid_close_ack_evidence(log.replace(outcome, "CLOSE_BEFORE_ACK"))
