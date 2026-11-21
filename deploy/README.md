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

```bash
export LICHESS_BOT_TOKEN="<ваш токен>"   # lichess-bot берёт токен из этой переменной
python lichess-bot.py
```

Бот принимает вызовы: стандартные шахматы, bullet/blitz/rapid (база до 30 мин, инкремент до 20 с),
rated и casual, от людей и ботов, по одной партии одновременно. Движку передаются
`Hash 256`, `Threads 1`, `Move Overhead 100`. Чтобы бот сам вызывал других ботов,
поставьте `matchmaking.allow_matchmaking: true` в `config.yml`.

## Проверка движка перед запуском

```bash
cd /c/dev/chessbot
./bins/best.exe bench 10        # должен отработать без ошибок
```

## Примечания

- Движок однопоточный (`Threads` > 1 игнорируется), без пондеринга.
- Если бот теряет время в bullet — увеличьте `Move Overhead` в `uci_options`
  (миллисекунды) и/или `move_overhead` lichess-bot.
- Бинарник собран с `target-cpu=native`: на другом компьютере пересоберите его
  (`cargo build --release`), иначе возможна ошибка «illegal instruction».
