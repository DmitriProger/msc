# msc — Minecraft Server Control

Профессиональная CLI-утилита для управления Minecraft-серверами на Linux.
Написана на Rust. Красивый TUI, автовосстановление при ребуте.

## Быстрый старт

```bash
msc install          # инициализация системы
msc                  # открыть панель управления (TUI)
```

## Установка

```bash
cargo build --release
sudo cp target/release/msc /usr/local/bin/msc
sudo msc install
```

## Структура серверов

```
/opt/minecraft/
├── lobby/
│   ├── start.sh     # обязателен
│   └── msc.toml     # опционален (лимиты, настройки)
└── survival/
    └── start.sh
```

## Команды

```
msc                          # TUI панель управления
msc <name>                   # TUI конкретного сервера
msc <name> start             # запустить сервер
msc <name> stop              # остановить сервер
msc <name> restart           # рестарт
msc <name> console           # подключиться к консоли
msc <name> status            # статус (для скриптов)
msc <name> send "<command>"  # отправить команду в консоль
msc list                     # список всех серверов
msc install                  # установить systemd watchdog
msc version                  # версия
```

## Конфигурация

`/etc/msc/config.toml` — глобальная конфигурация  
`/opt/minecraft/<name>/msc.toml` — настройки на сервер

## Разработка

```bash
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt
```

## Лицензия

MIT
