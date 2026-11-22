import unittest

from scripts.playtest import loot_contest_evidence


def client(player, owned):
    return (f"INFO LOOT CONTEST pressed raid=8 item=9 player={player}\n"
            f"INFO LOOT CONTEST converged raid=8 item=9 player={player} owned={owned} ground=0 model=0\n")


class LootContestEvidenceTests(unittest.TestCase):
    winner = "INFO LOOT CONTEST received raid=8 item=9 player=11\n"
    loser = "INFO LOOT CONTEST received raid=8 item=9 player=1\n"
    commit = "INFO LOOT CONTEST committed raid=8 item=9 player=11\n"

    def test_both_attempts_and_one_owner_without_requiring_same_tick(self):
        result = loot_contest_evidence((client(1, 0), client(11, 2)), self.winner + self.commit + self.loser)
        self.assertEqual(result["attempts"], [1, 11])
        self.assertEqual(result["winner"], 11)
        self.assertEqual(result["owned_quantities"], {1: 0, 11: 2})

    def test_missing_attempt_wrong_target_or_model_still_present_fails(self):
        server = self.winner + self.commit + self.loser
        for logs, text in (
            ((client(1, 0), client(11, 1)), self.winner + self.commit),
            ((client(1, 0).replace("item=9", "item=10"), client(11, 1)), server),
            ((client(1, 0).replace("model=0", "model=1"), client(11, 1)), server),
            ((client(1, 0).splitlines()[1], client(11, 1)), server),
        ):
            with self.subTest(logs=logs, text=text), self.assertRaises(ValueError):
                loot_contest_evidence(logs, text)

    def test_two_commits_or_disagreeing_ownership_fails(self):
        server = self.winner + self.commit + self.loser
        for logs, text in (
            ((client(1, 0), client(11, 1)), server + self.commit),
            ((client(1, 1), client(11, 1)), server),
            ((client(1, 1), client(11, 0)), server),
            ((client(1, 0), client(11, 1)), self.commit + self.winner + self.loser),
        ):
            with self.subTest(logs=logs, text=text), self.assertRaises(ValueError):
                loot_contest_evidence(logs, text)
