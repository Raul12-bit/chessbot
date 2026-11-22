# Запуск Chessbot на Lichess (BOT-аккаунт)

Движок общается с Lichess через официальный клиент
[lichess-bot](https://github.com/lichess-bot-devs/lichess-bot) по протоколу UCI.

## 1. Новый аккаунт

1. Создайте **новый** аккаунт на lichess.org (выйдите из основного или используйте приватное окно).
   Важно: аккаунт, который будет переведён в BOT, **не должен иметь сыгранных партий** —
   апгрейд в BOT необратим, и для аккаунта с партиями он запрещён.
2. Подтвердите e-mail.

## 2. API-токен

1. Войдите в новый аккаунт и откройте <https://lichess.org/account/oauth/token/create>.
2. Отметьте scope **`bot:play`** («Play games with the bot API»), задайте имя токена, создайте.
3. Скопируйте токен (показывается один раз). **Не коммитьте его** и не вставляйте в файлы,
   которые попадают в git. Храните только в переменной окружения или в локальном
   `config.yml` (он в `.gitignore`).

## 3. Установка lichess-bot

### PowerShell

Активировать venv не обязательно: достаточно вызывать `venv\Scripts\python.exe` напрямую.
Так не придётся менять ExecutionPolicy, которая обычно блокирует `Activate.ps1`.

```powershell
cd C:\dev\chessbot\deploy
git clone https://github.com/lichess-bot-devs/lichess-bot.git
cd lichess-bot
python -m venv venv
.\venv\Scripts\python.exe -m pip install -r requirements.txt
New-Item -ItemType Directory -Force engines
Copy-Item ..\..\bins\best.exe engines\chessbot.exe
Copy-Item ..\config.yml.example config.yml
```

Апгрейд в BOT и запуск в PowerShell (переменная живёт только в текущем окне):

```powershell
$env:LICHESS_BOT_TOKEN = "<ваш токен>"
.\venv\Scripts\python.exe lichess-bot.py -u   # один раз, необратимо (см. раздел 4)
.\venv\Scripts\python.exe lichess-bot.py      # запуск бота
```

### Git Bash

```bash
cd /c/dev/chessbot/deploy
git clone https://github.com/lichess-bot-devs/lichess-bot.git
cd lichess-bot
python -m venv venv
source venv/Scripts/activate
pip install -r requirements.txt
mkdir -p engines
cp ../../bins/best.exe engines/chessbot.exe
cp ../../book/gm_book.bin engines/book.bin
cp ../config.yml.example config.yml
```

(`deploy/lichess-bot/` и `deploy/config.yml` перечислены в `.gitignore`.)

## 4. Апгрейд аккаунта до BOT

```bash
export LICHESS_BOT_TOKEN="<ваш токен>"
python lichess-bot.py -u
```

Флаг `-u` один раз переводит аккаунт в BOT (необратимо). Альтернатива — запрос
`POST https://lichess.org/api/bot/account/upgrade` с заголовком `Authorization: Bearer <токен>`.

## 5. Запуск

### Проще всего: `start_bot.bat` (с выбором режима)

Дважды щёлкните `deploy\start_bot.bat` (или из PowerShell: `C:\dev\chessbot\deploy\start_bot.bat`).
Скрипт спросит, какие партии бот будет искать сам:

| № | Режим | Контроли |
|---|---|---|
| 1 | Пуля | 1+0, 1+1, 2+1 |
| 2 | Блиц | 3+0, 3+2, 5+0, 5+3 |
| 3 | Рапид | 10+0, 10+5, 15+10 |
| 4 | Пуля + блиц | 1+0 … 5+3 |
| 5 | Всё вперемешку | все перечисленные |
| 6 | Без поиска | только принимает вызовы |

Режим можно передать сразу: `start_bot.bat blitz` (`bullet`, `blitz`, `rapid`, `fast`, `mixed`, `none`).

Второй вопрос — каких соперников искать:

| № | Соперники | Что даёт |
|---|---|---|
| 1 | равные (±300 от рейтинга бота) | честная проверка силы, ~50 % очков при «правильном» рейтинге |
| 2 | слабее (в пределах 400, предпочитать слабых) | больше побед, медленный рост рейтинга |
| 3 | сильнее (в пределах 400, предпочитать сильных) | больше поражений, быстрее рост рейтинга |

Оба аргумента сразу: `start_bot.bat fast weaker`. Процент побед на Lichess определяется прежде всего
разницей рейтингов: против равных по рейтингу соперников любой бот набирает около 50 %. 80–90 % побед —
это соперники примерно на 250–400 пунктов слабее. Учтите: команды-лидерборды ботов на Lichess не
принимают ботов, которые в основном «фармят» слабых соперников.
Если переменная `LICHESS_BOT_TOKEN` не задана, скрипт спросит токен (ввод скрыт, на диск не пишется).

Как ищутся партии: через ~15 с после окончания партии бот вызывает случайного бота подходящего рейтинга;
если тот отказал или не ответил — следующий вызов через ~20 с, и так пока кто-то не примет (обычно 1–3 минуты:
многие боты отказываются, заняты или исчерпали дневной лимит — строки `declined` в логе, это нормально).
Во время партии бот никого не вызывает (одна партия за раз). Сам lichess-bot ждёт минимум 1 минуту после партии
и 60 с между вызовами; `start_bot` сокращает эти паузы при запуске через `deploy/run_lichess_bot.py`,
не меняя файлы lichess-bot (при ответе Lichess «слишком много запросов» lichess-bot сам делает паузу).
Боты, которые отказали, повторно таким же вызовом не беспокоятся. Lichess разрешает боту не больше
100 партий в день против других ботов. Вызовы от людей и ботов принимаются как раньше (по `config.yml`).
Пункт «Без поиска» (6) выключает поиск полностью — бот только ждёт вызовов.

Скрипт не меняет `config.yml`: он пишет `lichess-bot/config_run.yml` и запускает бота с ним.

### Вручную

```bash
export LICHESS_BOT_TOKEN="<ваш токен>"   # lichess-bot берёт токен из этой переменной
python lichess-bot.py
```

Бот принимает вызовы: стандартные шахматы, bullet/blitz/rapid (база до 30 мин, инкремент до 20 с),
rated и casual, от людей и ботов, по одной партии одновременно. Движку передаются
`Hash 256`, `Threads 8`, `Move Overhead 100`. Чтобы бот сам вызывал других ботов,
поставьте `matchmaking.allow_matchmaking: true` в `config.yml`.

## Проверка движка перед запуском

```bash
cd /c/dev/chessbot
./bins/best.exe bench 10        # должен отработать без ошибок
```

## Примечания

- Многопоточный поиск (Lazy SMP): `Threads 8` в `uci_options`. На этом ноутбуке (i5-12450H: 8 ядер — 4P+4E, 12 потоков)
  это оставляет запас системе и lichess-bot. Если компьютер нужен для другой работы — уменьшите до 4.
  Пондеринг включён (`ponder: true` в секции `engine`): движок думает и во время хода соперника.
- Дебютная книга из ~59 тыс. партий гроссмейстеров (`book/gm_book.bin`, ходы Карлсена с весом ×3):
  движок сам берёт `book.bin` из папки, где лежит `chessbot.exe`. Отключить — `OwnBook: false`
  в `uci_options`; другой файл — `BookFile: <путь>`. Книга используется только в партиях с часами.
- Таблицы эндшпиля: включены онлайн-таблицы Lichess (`online_moves.online_egtb`, до 7 фигур, если на часах
  ≥ 5 с). Это база идеально разыгранных окончаний, а не чужой движок; в такой позиции ход берётся из базы.
  Отключить — `enabled: false` в этой секции.
- Если бот теряет время в bullet — увеличьте `Move Overhead` в `uci_options`
  (миллисекунды) и/или `move_overhead` lichess-bot.
- Бинарник собран с `target-cpu=native`: на другом компьютере пересоберите его
  (`cargo build --release`), иначе возможна ошибка «illegal instruction».
