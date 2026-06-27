# Anvil

Профессиональная CLI-утилита для управления Minecraft-серверами на Linux. Написана на Rust: TUI-панель, реальные cgroup-лимиты RAM/CPU, автозапуск после ребута, бекапы.

```
┌ Anvil ───────────────────────────────────────────┐
│ servers 3 total  2 up  1 down                     │
│ ram     6.1G used  /  24G host                    │
│ host    16 cores                                  │
└───────────────────────────────────────────────────┘
┌ Servers ──────────────────────────────────────────┐
│   Server      Status   CPU    RAM         Uptime  │
│ > lobby       ONLINE   12%    3.8G / 4G    2d 4h  │
│   survival    ONLINE   34%    2.3G / 8G    2d 4h  │
│   creative    OFFLINE  -      -            -      │
└───────────────────────────────────────────────────┘
```

---

## Установка

**Из GitHub Release** (рекомендуется):

```bash
# x86_64 (для ARM64 — anvil-linux-aarch64)
curl -fsSL https://github.com/DmitriProger/msc/releases/latest/download/anvil-linux-x86_64 -o anvil
chmod +x anvil
sudo mv anvil /usr/local/bin/
```

**Из исходников:**

```bash
git clone https://github.com/DmitriProger/msc.git && cd msc
cargo build --release
sudo install -m 755 target/release/anvil /usr/local/bin/anvil
```

**Инициализация системы** (создаёт пользователя `minecraft`, директории, конфиг и systemd-watchdog):

```bash
sudo anvil install
```

---

## Быстрый старт

```bash
# 1. Папка сервера + скрипт запуска
mkdir -p /opt/minecraft/lobby
nano /opt/minecraft/lobby/start.sh
chmod +x /opt/minecraft/lobby/start.sh

# 2. Запустить и открыть панель
anvil lobby start
anvil
```

Пример `start.sh` (флаги JVM пишешь сам — Anvil их не трогает):

```bash
#!/bin/bash
cd /opt/minecraft/lobby
java -Xms3G -Xmx3G -jar paper.jar --nogui
```

> `-Xmx` должен быть **ниже** `memory_max` из `anvil.toml` (запас ~1G на оверхед JVM) — иначе сервер словит OOM-killer. Anvil предупредит при старте, если заметит превышение.

---

## Структура

```
/opt/minecraft/<server>/
├── start.sh        ← обязателен (без него папка не считается сервером)
├── anvil.toml      ← опционален (лимиты, авторестарт, бекапы)
└── backups/        ← локальные бекапы (создаёт Anvil)

/etc/anvil/config.toml      ← глобальный конфиг
/var/lib/anvil/state.json   ← желаемое состояние (watchdog читает при ребуте)
```

Имя папки сервера: `^[a-z0-9_-]{1,64}$` (строчные буквы, цифры, `-`, `_`).

---

## Конфигурация сервера — `anvil.toml`

Опционален; без него — дефолты. Читается при каждой операции, перезапуск не нужен.

```toml
[limits]
memory_max   = "4G"      # жёсткий потолок RAM (cgroup MemoryMax). Дефолт 4G
cpu_cores    = 2         # лимит CPU (cgroup CPUQuota = cpu_cores × 100%). Дефолт 2
cpu_affinity = "0,1"     # привязка к ядрам (cgroup AllowedCPUs). Опционально

[server]
description          = "Лобби"   # отображается в TUI
auto_restart         = true      # поднимать при падении (watchdog). Дефолт true
restart_delay_secs   = 5         # пауза перед рестартом. Дефолт 5
max_restart_attempts = 3         # максимум попыток подряд. Дефолт 3

[backup]
enabled     = true
keep_last   = 7              # сколько последних архивов хранить. Дефолт 7
stop_server = true           # true: стоп→архив→старт; false: горячий бекап без даунтайма
include     = ["world/", "world_nether/", "world_the_end/", "server.properties"]
exclude     = ["*.log", "logs/", "cache/"]
```

### Как работают лимиты ресурсов

Лимиты применяются **уровнем выше процесса** — через cgroup v2, а не флагами JVM. При старте сервер запускается в transient-scope:

```
systemd-run --scope -p MemoryMax=<memory_max> -p MemoryHigh=<~85%> \
            -p CPUQuota=<cpu_cores×100>% [-p AllowedCPUs=<cpu_affinity>] ./start.sh
```

- `memory_max` — **жёсткая стена ядра** для всего процесса JVM (heap + Metaspace + Netty + GC), не только кучи. Превышение → OOM-killer.
- Под не-root пользователем используется `--scope --user` — для этого включи linger: `sudo loginctl enable-linger <user>`.
- Если `systemd-run` недоступен — деградация на `taskset` (только affinity) с записью в лог, что лимиты не применены (старт не ломается).
- `cpu_affinity` валидируется как CPU-list (`0,1`, `3-6`); некорректное значение игнорируется.

---

## Конфигурация глобальная — `/etc/anvil/config.toml`

```toml
language     = "en"               # en | ru
servers_root = "/opt/minecraft"   # корень всех серверов
log_level    = "info"             # trace | debug | info | warn | error
tmux_socket  = "anvil"            # имя tmux-сокета

[backup]
gdrive_folder = "Anvil Backups"
token_path    = "/var/lib/anvil/gdrive_token.json"

[update]
repo = "DmitriProger/msc"         # GitHub repo с релизами
```

---

## Команды

```bash
anvil                          # TUI-панель со всеми серверами
anvil <name>                   # TUI конкретного сервера
anvil list                     # список в stdout (для скриптов)

anvil <name> start             # запуск (с cgroup-лимитами)
anvil <name> stop              # graceful: console stop → SIGTERM → SIGKILL (таймаут 60с)
anvil <name> restart
anvil <name> console           # консоль (tmux attach; выход — Ctrl-B, затем D)
anvil <name> status            # машиночитаемый статус
anvil <name> send "<cmd>"      # отправить команду в консоль без входа

anvil <name> backup            # создать бекап
anvil <name> backup list       # список локальных бекапов
anvil <name> backup restore <file>

anvil install | uninstall      # systemd-watchdog
anvil update [--check]         # обновить бинарник из GitHub Release
anvil version
```

В TUI: `j`/`k` или стрелки — выбор, `Enter` — открыть/старт, `S` — стоп, `R` — рестарт, `C` — консоль, `Q` — выход.

---

## Бекапы

Создаются **локально** в `<server>/backups/<name>-<timestamp>.zip`; при настроенном Google Drive дополнительно заливаются туда (локальная копия остаётся всегда).

```bash
anvil lobby backup                              # создать
anvil lobby backup list                         # список
anvil lobby backup restore lobby-20240115-040000.zip
```

- Онлайн + `stop_server = true` → стоп → архив → старт.
- Онлайн + `stop_server = false` → горячий бекап: `save-off` → `save-all` → архив → `save-on`.
- Учитываются `include`/`exclude`; `backups/` всегда исключается; ротация по `keep_last`.

**Google Drive (опционально):** получи OAuth2 Client ID/Secret в [Google Cloud Console](https://console.cloud.google.com/) (тип *TV and Limited Input devices*), затем:

```bash
export GDRIVE_CLIENT_ID="..." GDRIVE_CLIENT_SECRET="..."
anvil backup auth      # пройти device-flow в браузере
anvil backup status    # проверить
```

**Расписание:** поле `schedule` пока **не запускается автоматически** (планировщик не подключён). Используй системный cron:

```cron
0 4 * * *  minecraft  /usr/local/bin/anvil lobby backup
```

---

## Требования и сборка

- Linux с **systemd и cgroup v2** (Ubuntu 22.04+ / Debian 12+) — `systemd-run` нужен для enforcement лимитов
- **tmux** (консоль и отправка команд)
- Rust 1.75+ (только для сборки)

```bash
cargo build --release          # релиз-бинарник
cargo test                     # тесты
cargo clippy -- -D warnings    # линтер
```

Отладка:

```bash
ANVIL_LOG=debug anvil lobby start        # подробные логи
sudo journalctl -u anvil-watchdog -f     # логи watchdog
```

Релиз: тег `v*` пушится в репозиторий → GitHub Actions собирает бинарники под 4 платформы и публикует Release.

---

## Лицензия

MIT
