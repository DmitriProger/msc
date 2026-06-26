# msc — Minecraft Server Control

Профессиональная CLI-утилита для управления Minecraft-серверами на Linux.  
Написана на Rust. Красивый TUI, автовосстановление при ребуте, бекапы на Google Drive.

---

## Содержание

- [Установка](#установка)
- [Быстрый старт](#быстрый-старт)
- [Структура серверов](#структура-серверов)
- [Конфигурация](#конфигурация)
  - [Глобальный конфиг /etc/msc/config.toml](#глобальный-конфиг-etcmsconfigtoml)
  - [Конфиг сервера msc.toml](#конфиг-сервера-msctoml)
- [Команды](#команды)
- [Бекапы](#бекапы)
- [Разработка](#разработка)

---

## Установка

### Из исходников

```bash
git clone https://github.com/DmitriProger/msc.git
cd msc
cargo build --release
sudo install -m 755 target/release/msc /usr/local/bin/msc
```

### Инициализация системы

```bash
sudo msc install
```

Команда сделает всё автоматически:
- Создаст пользователя `minecraft`
- Создаст директории `/opt/minecraft`, `/var/lib/msc`, `/var/log/msc`, `/etc/msc`
- Запишет конфиг `/etc/msc/config.toml` с дефолтами
- Установит и запустит systemd-сервис `msc-watchdog`

---

## Быстрый старт

```bash
# Создать первый сервер
mkdir /opt/minecraft/lobby
nano /opt/minecraft/lobby/start.sh     # написать команду запуска
chmod +x /opt/minecraft/lobby/start.sh

# Открыть панель управления
msc

# Или сразу запустить сервер
msc lobby start
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
│   ├── msc.toml             ← опционален (лимиты, авторестарт, бекапы)
│   ├── paper.jar
│   └── server.properties
├── survival/
│   ├── start.sh
│   └── msc.toml
└── creative/
    └── start.sh

/etc/msc/
└── config.toml              ← глобальный конфиг msc

/var/lib/msc/
├── state.json               ← кто должен быть online (watchdog читает при ребуте)
└── gdrive_token.json        ← OAuth2-токен Google Drive (после msc backup auth)

/var/log/msc/
└── msc.log                  ← логи watchdog и операций
```

**Правило обнаружения:** папка считается сервером если в ней есть `start.sh`.  
Имя папки должно соответствовать паттерну `^[a-z0-9_-]{1,64}$` (только строчные буквы, цифры, дефис, подчёркивание).

---

## Конфигурация

### Глобальный конфиг `/etc/msc/config.toml`

Создаётся автоматически при `msc install`. Редактировать от root:

```bash
sudo nano /etc/msc/config.toml
```

```toml
servers_root = "/opt/minecraft"   # папка со всеми серверами
log_level    = "info"             # уровень логов: trace | debug | info | warn | error
tmux_socket  = "msc"             # имя tmux-сокета (tmux -L msc ...)

[backup]
gdrive_folder = "MSC Backups"              # имя корневой папки на Google Drive
token_path    = "/var/lib/msc/gdrive_token.json"
tmp_dir       = "/var/lib/msc/tmp"
```

**Когда менять:**
| Параметр | Когда |
|---|---|
| `servers_root` | Хочешь хранить серверы не в `/opt/minecraft` |
| `log_level` | При отладке поставь `debug` или `trace` |
| `tmux_socket` | Если уже используешь tmux-сокет с именем `msc` |
| `gdrive_folder` | Хочешь другое имя папки на Google Drive |

После изменения перезапускать `msc` не нужно — конфиг читается при каждом вызове.  
Watchdog перезапустить: `sudo systemctl restart msc-watchdog`

---

### Конфиг сервера `msc.toml`

Создаётся вручную в папке каждого сервера. Полностью опционален — без него используются дефолты.

```bash
nano /opt/minecraft/lobby/msc.toml
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

После изменения `msc.toml` перезапускать ничего не нужно — файл читается при каждой операции.

---

## Команды

### Управление серверами

```bash
msc                          # открыть TUI-панель со списком всех серверов
msc <name>                   # открыть TUI конкретного сервера
msc list                     # список серверов в stdout (для скриптов)

msc <name> start             # запустить сервер
msc <name> stop              # остановить сервер (graceful: stop → SIGTERM → SIGKILL)
msc <name> restart           # рестарт (stop + start)
msc <name> console           # подключиться к консоли (tmux attach)
msc <name> status            # статус в машиночитаемом формате
msc <name> send "<command>"  # отправить команду в консоль без входа
```

### Примеры

```bash
# Отправить сообщение игрокам без входа в консоль
msc lobby send "say Server restarting in 1 minute!"

# Принудительно сохранить мир
msc lobby send "save-all"

# Проверить статус в скрипте
msc lobby status | grep "^status:" | awk '{print $2}'
```

### Выход из консоли

При `msc <name> console` открывается tmux-сессия.  
Выход: **Ctrl-B, затем D** (стандартный detach tmux). После этого TUI вернётся обратно.

### Системное

```bash
msc install                  # полная инициализация системы
msc uninstall                # удалить systemd watchdog
msc version                  # версия
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
msc backup auth
```

3. Следуй инструкции: открой ссылку в браузере, введи код.

4. Проверь подключение:
```bash
msc backup status
```

### Запустить бекап вручную

```bash
msc lobby backup
```

### Список бекапов на Drive

```bash
msc lobby backup list
```

### Восстановление из бекапа

```bash
msc lobby backup restore lobby_2024-01-15_04-00-00.zip
```

Сервер будет остановлен на время восстановления и запущен обратно после.

### Расписание

Задаётся в `msc.toml` через cron-выражение. Watchdog запускает бекапы автоматически — системный cron не нужен.

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

### Обновить бинарник на сервере

```bash
cd /path/to/msc
git pull origin dev
cargo build --release
sudo install -m 755 target/release/msc /usr/local/bin/msc

# Если менялся watchdog — перезапустить
sudo systemctl restart msc-watchdog
```

### Отладка

```bash
# Включить подробные логи для конкретного запуска
MSC_LOG=debug msc lobby start

# Полный трейс
MSC_LOG=trace msc lobby start

# Логи watchdog в реальном времени
sudo journalctl -u msc-watchdog -f
```

---

## Лицензия

MIT
