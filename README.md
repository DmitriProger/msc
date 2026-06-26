# Anvil

Anvil - профессиональная CLI-утилита для управления Minecraft-серверами на Linux.  
Написана на Rust. Красивый TUI, автовосстановление при ребуте, бекапы на Google Drive.

---

## Содержание

- [Установка](#установка)
- [Обновление и миграция](#обновление-и-миграция)
- [Быстрый старт](#быстрый-старт)
- [Структура серверов](#структура-серверов)
- [Конфигурация](#конфигурация)
  - [Глобальный конфиг /etc/anvil/config.toml](#глобальный-конфиг-etcanvilconfigtoml)
  - [Конфиг сервера anvil.toml](#конфиг-сервера-anviltoml)
- [Команды](#команды)
- [Бекапы](#бекапы)
- [Разработка](#разработка)

---

## Установка

### Из исходников

```bash
git clone https://github.com/DmitriProger/anvil.git
cd anvil
cargo build --release
sudo install -m 755 target/release/anvil /usr/local/bin/anvil
```

### Инициализация системы

```bash
sudo anvil install
```

Команда сделает всё автоматически:
- Создаст пользователя `minecraft`
- Создаст директории `/opt/minecraft`, `/var/lib/anvil`, `/var/log/anvil`, `/etc/anvil`
- Запишет конфиг `/etc/anvil/config.toml` с дефолтами
- Установит и запустит systemd-сервис `anvil-watchdog`

---

## Обновление и миграция

### Обычное обновление Anvil

Когда Anvil уже установлен и для версии есть GitHub Release:

```bash
sudo anvil update --check
sudo anvil update
```

`anvil update` заменяет только бинарник `/usr/local/bin/anvil`. Minecraft-серверы и tmux-сессии не останавливаются.

Если менялась логика watchdog:

```bash
sudo systemctl restart anvil-watchdog
```

### Миграция со старого msc на Anvil

Полный rename меняет системные пути, unit, tmux socket и session prefix:

| Было | Стало |
|---|---|
| `/usr/local/bin/msc` | `/usr/local/bin/anvil` |
| `/etc/msc/config.toml` | `/etc/anvil/config.toml` |
| `/var/lib/msc` | `/var/lib/anvil` |
| `/var/log/msc` | `/var/log/anvil` |
| `msc-watchdog` | `anvil-watchdog` |
| `msc_<server>` | `anvil_<server>` |
| `msc.toml` | `anvil.toml` |
| `MSC_LOG` | `ANVIL_LOG` |

Остановить старый watchdog можно без остановки Minecraft-серверов:

```bash
sudo systemctl disable --now msc-watchdog || true
```

Установить новый бинарник:

```bash
git pull origin dev
cargo build --release
sudo install -m 755 target/release/anvil /usr/local/bin/anvil
```

Перенести глобальный конфиг, если он был:

```bash
sudo mkdir -p /etc/anvil
sudo cp /etc/msc/config.toml /etc/anvil/config.toml
sudo sed -i \
  -e 's#/var/lib/msc#/var/lib/anvil#g' \
  -e 's#/var/log/msc#/var/log/anvil#g' \
  -e 's#MSC Backups#Anvil Backups#g' \
  -e 's#tmux_socket *= *"msc"#tmux_socket = "anvil"#g' \
  /etc/anvil/config.toml
```

Переименовать серверные конфиги:

```bash
find /opt/minecraft -name msc.toml -exec sh -c 'mv "$1" "$(dirname "$1")/anvil.toml"' _ {} \;
```

Инициализировать новые директории и watchdog:

```bash
sudo anvil install
```

Важно: уже запущенные старые tmux-сессии `msc_<server>` продолжат работать, но Anvil их не считает своими. Чтобы Anvil начал управлять сервером, в ближайшее окно обслуживания останови старую сессию через старый socket и запусти сервер через Anvil:

```bash
tmux -L msc attach -t msc_lobby
# внутри консоли Minecraft: stop

anvil lobby start
```

После проверки можно удалить старый бинарник и старые служебные файлы:

```bash
sudo rm -f /usr/local/bin/msc
sudo rm -f /etc/systemd/system/msc-watchdog.service
sudo rm -rf /etc/msc /var/lib/msc /var/log/msc
sudo systemctl daemon-reload
```

---

## Быстрый старт

```bash
# Создать первый сервер
mkdir /opt/minecraft/lobby
nano /opt/minecraft/lobby/start.sh     # написать команду запуска
chmod +x /opt/minecraft/lobby/start.sh

# Открыть панель управления
anvil

# Или сразу запустить сервер
anvil lobby start
```

### Пример start.sh для Paper/Vanilla

```bash
#!/bin/bash
cd /opt/minecraft/lobby
java -Xmx4G -Xms1G -jar paper.jar --nogui
```

---

## Структура серверов

```
/opt/minecraft/              ← корень всех серверов
├── lobby/
│   ├── start.sh             ← обязателен (без него папка игнорируется)
│   ├── anvil.toml             ← опционален (лимиты, авторестарт, бекапы)
│   ├── paper.jar
│   └── server.properties
├── survival/
│   ├── start.sh
│   └── anvil.toml
└── creative/
    └── start.sh

/etc/anvil/
└── config.toml              ← глобальный конфиг anvil

/var/lib/anvil/
├── state.json               ← кто должен быть online (watchdog читает при ребуте)
└── gdrive_token.json        ← OAuth2-токен Google Drive (после anvil backup auth)

/var/log/anvil/
└── anvil.log                  ← логи watchdog и операций
```

**Правило обнаружения:** папка считается сервером если в ней есть `start.sh`.  
Имя папки должно соответствовать паттерну `^[a-z0-9_-]{1,64}$` (только строчные буквы, цифры, дефис, подчёркивание).

---

## Конфигурация

### Глобальный конфиг `/etc/anvil/config.toml`

Создаётся автоматически при `anvil install`. Редактировать от root:

```bash
sudo nano /etc/anvil/config.toml
```

```toml
language = "en"                     # язык интерфейса: en | ru
servers_root = "/opt/minecraft"   # папка со всеми серверами
log_level    = "info"             # уровень логов: trace | debug | info | warn | error
tmux_socket  = "anvil"             # имя tmux-сокета (tmux -L anvil ...)

[backup]
gdrive_folder = "Anvil Backups"              # имя корневой папки на Google Drive
token_path    = "/var/lib/anvil/gdrive_token.json"
tmp_dir       = "/var/lib/anvil/tmp"

[update]
repo = "DmitriProger/anvil"         # GitHub repo с релизами для anvil update
```

**Когда менять:**
| Параметр | Когда |
|---|---|
| `language` | `en` для английского интерфейса, `ru` для русского |
| `servers_root` | Хочешь хранить серверы не в `/opt/minecraft` |
| `log_level` | При отладке поставь `debug` или `trace` |
| `tmux_socket` | Если уже используешь tmux-сокет с именем `anvil` |
| `gdrive_folder` | Хочешь другое имя папки на Google Drive |
| `update.repo` | Если релизы лежат не в `DmitriProger/anvil` |

После изменения перезапускать `anvil` не нужно — конфиг читается при каждом вызове.  
Watchdog перезапустить: `sudo systemctl restart anvil-watchdog`

---

### Конфиг сервера `anvil.toml`

Создаётся вручную в папке каждого сервера. Полностью опционален — без него используются дефолты.

```bash
nano /opt/minecraft/lobby/anvil.toml
```

```toml
[limits]
memory_max   = "4G"      # максимум RAM (отображается в TUI)
cpu_cores    = 2         # количество ядер
cpu_affinity = "0,1"     # привязка к конкретным ядрам через taskset (опционально)

[server]
description          = "Лобби-сервер"   # отображается в TUI
auto_restart         = true             # перезапускать при падении (watchdog)
restart_delay_secs   = 5               # пауза перед рестартом (секунды)
max_restart_attempts = 3               # максимум попыток подряд

[backup]
enabled        = true
schedule       = "0 4 * * *"   # cron: каждый день в 04:00
keep_last      = 7             # хранить последние N бекапов на Drive
archive_format = "zip"
stop_server    = true          # остановить сервер перед бекапом

# Что включить в бекап (если пусто — архивируется вся папка)
include = [
  "world/",
  "world_nether/",
  "world_the_end/",
  "server.properties",
  "ops.json",
  "whitelist.json",
]

# Что исключить (glob-паттерны, применяются поверх include)
exclude = [
  "*.log",
  "*.log.gz",
  "logs/",
  "cache/",
  "tmp/",
]
```

**Дефолтные значения** (если параметр не указан):

| Параметр | Дефолт |
|---|---|
| `memory_max` | `4G` |
| `cpu_cores` | `2` |
| `auto_restart` | `true` |
| `restart_delay_secs` | `5` |
| `max_restart_attempts` | `3` |
| `backup.enabled` | `false` |
| `backup.keep_last` | `7` |

После изменения `anvil.toml` перезапускать ничего не нужно — файл читается при каждой операции.

---

## Команды

### Управление серверами

```bash
anvil                          # открыть TUI-панель со списком всех серверов
anvil <name>                   # открыть TUI конкретного сервера
anvil list                     # список серверов в stdout (для скриптов)

anvil <name> start             # запустить сервер
anvil <name> stop              # остановить сервер (graceful: stop → SIGTERM → SIGKILL)
anvil <name> restart           # рестарт (stop + start)
anvil <name> console           # подключиться к консоли (tmux attach)
anvil <name> status            # статус в машиночитаемом формате
anvil <name> send "<command>"  # отправить команду в консоль без входа
```

### Примеры

```bash
# Отправить сообщение игрокам без входа в консоль
anvil lobby send "say Server restarting in 1 minute!"

# Принудительно сохранить мир
anvil lobby send "save-all"

# Проверить статус в скрипте
anvil lobby status | grep "^status:" | awk '{print $2}'
```

### Выход из консоли

При `anvil <name> console` открывается tmux-сессия.  
Выход: **Ctrl-B, затем D** (стандартный detach tmux). После этого TUI вернётся обратно.

### Системное

```bash
anvil install                  # полная инициализация системы
anvil uninstall                # удалить systemd watchdog
anvil version                  # версия
anvil update --check           # проверить наличие релиза новее текущей версии
anvil update                   # скачать и установить новый бинарник из GitHub Release
```

`anvil update` заменяет только бинарник `anvil`. Запущенные Minecraft-серверы и tmux-сессии не останавливаются.
Если менялась логика watchdog, после обновления можно отдельно выполнить:

```bash
sudo systemctl restart anvil-watchdog
```

---

## Бекапы

### Первоначальная настройка

1. Получи OAuth2-credentials в [Google Cloud Console](https://console.cloud.google.com/):
   - Создай проект → APIs & Services → Credentials
   - OAuth 2.0 Client ID → тип: **TV and Limited Input devices**
   - Скопируй Client ID и Client Secret

2. Авторизуйся:
```bash
export GDRIVE_CLIENT_ID="your-client-id"
export GDRIVE_CLIENT_SECRET="your-client-secret"
anvil backup auth
```

3. Следуй инструкции: открой ссылку в браузере, введи код.

4. Проверь подключение:
```bash
anvil backup status
```

### Запустить бекап вручную

```bash
anvil lobby backup
```

### Список бекапов на Drive

```bash
anvil lobby backup list
```

### Восстановление из бекапа

```bash
anvil lobby backup restore lobby_2024-01-15_04-00-00.zip
```

Сервер будет остановлен на время восстановления и запущен обратно после.

### Расписание

Задаётся в `anvil.toml` через cron-выражение. Watchdog запускает бекапы автоматически — системный cron не нужен.

```toml
schedule = "0 4 * * *"      # каждый день в 04:00
schedule = "0 */6 * * *"    # каждые 6 часов
schedule = "0 3 * * 0"      # каждое воскресенье в 03:00
```

---

## Разработка

### Требования

- Rust 1.75+
- tmux
- Linux (Ubuntu 22.04+ / Debian 12+)

### Сборка и проверка

```bash
cargo build                    # debug-сборка
cargo build --release          # release-сборка (~5-8 МБ, статически слинкован)
cargo test                     # все тесты
cargo clippy -- -D warnings    # линтер (должен быть чистым)
cargo fmt                      # форматирование
```

### Структура веток

```
main    ← только стабильный код, только через PR
dev     ← основная ветка разработки
feat/*  ← новый функционал
fix/*   ← исправление багов
```

### Формат коммитов

```
feat(tui):      add main server list screen
fix(state):     atomic write race condition on rename
refactor(tmux): extract session check to helper
chore(deps):    update ratatui to 0.29
docs:           update README with backup instructions
```

### Как добавить изменение

```bash
# 1. Создать ветку от dev
git checkout dev
git checkout -b feat/my-feature

# 2. Внести изменения, убедиться что всё проходит
cargo fmt
cargo clippy -- -D warnings
cargo test

# 3. Закоммитить
git add .
git commit -m "feat(scope): описание на английском"

# 4. Запушить и создать PR в dev
git push origin feat/my-feature
# → открыть PR на GitHub: feat/my-feature → dev
```

### Выпустить релиз

```bash
git checkout main
git pull
git tag v1.0.1
git push origin v1.0.1
```

GitHub Actions соберёт release assets:

```text
anvil-linux-x86_64
anvil-linux-aarch64
anvil-macos-x86_64
anvil-macos-aarch64
anvil-windows-x86_64.exe
```

После этого на сервере:

```bash
sudo anvil update
```

### Отладка

```bash
# Включить подробные логи для конкретного запуска
ANVIL_LOG=debug anvil lobby start

# Полный трейс
ANVIL_LOG=trace anvil lobby start

# Логи watchdog в реальном времени
sudo journalctl -u anvil-watchdog -f
```

---

## Лицензия

MIT
