"""Run lichess-bot with faster matchmaking (used by start_bot.py; lichess-bot files stay untouched).

lichess-bot waits `challenge_timeout` minutes (at least 1) after a game and 60 seconds between
challenges. Many bots decline or ignore challenges, so finding a game took several minutes.
Here, at runtime only:
  - first challenge 15 s after a game ends (or after start),
  - next challenge 20 s after the previous one was declined / expired.
Only one challenge is open at a time (lichess-bot waits for an answer or the 20-25 s expiry),
and lichess-bot still backs off on HTTP 429 (rate limit) responses.

Must be started with the lichess-bot folder as the working directory:
    venv\\Scripts\\python.exe ..\\run_lichess_bot.py --config config_run.yml
"""
import os
import sys

# lichess-bot's `lib` package lives in the working directory (also needed in spawned processes)
sys.path.insert(0, os.getcwd())

from lib import matchmaking  # noqa: E402
from lib.timer import Timer, seconds  # noqa: E402

POST_GAME_SECONDS = 15
BETWEEN_CHALLENGES_SECONDS = 20

_original_init = matchmaking.Matchmaking.__init__


def _faster_init(self, *args, **kwargs):
    _original_init(self, *args, **kwargs)
    self.min_wait_time = seconds(BETWEEN_CHALLENGES_SECONDS)
    self.last_game_ended_delay = Timer(seconds(POST_GAME_SECONDS))


matchmaking.Matchmaking.__init__ = _faster_init

if __name__ == "__main__":
    from lib.lichess_bot import start_program

    start_program()
