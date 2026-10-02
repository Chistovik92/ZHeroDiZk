# Структура проекта

Автор: SecretHero. Описана реальная структура на версию 0.1.3. Каталоги будущих
компонентов помечены «планируется» и в репозитории **ещё не существуют**.

```text
ZHeroDiZk/
├── Cargo.toml              Rust workspace; единый номер версии проекта
├── LICENSE                 полный текст AGPL-3.0 (не изменять)
├── NOTICE                  авторство и обязательные уведомления
├── AUTHORS                 авторы
├── CHANGELOG.md            история изменений по версиям
├── brand.json              название, репозиторий, статус публикации
├── upstream.lock.json      закреплённый исходный клиент (URL, тег, полный SHA)
├── .gitattributes          окончания строк LF во всём репозитории
├── .github/workflows/
│   ├── checks.yml          проверки на каждый push/PR: Python-тесты, lock, Rust-тесты
│   └── client-linux.yml    полная сборка Linux-клиента (запуск вручную)
├── client/
│   └── linux-identity.json манифест замен идентичности Linux-клиента
├── crates/
│   └── access-policy/      Rust-библиотека правил доступа (zherodizk-access-policy)
├── tools/                  служебные скрипты (Python, только stdlib)
│   ├── doctor.py           диагностика Linux/X11, проверка libxdo
│   ├── bootstrap.py        получение закреплённого исходного клиента в НОВЫЙ каталог
│   ├── prepare_client.py   проверка чистоты и подготовка checkout к сборке
│   ├── apply_patch_once.py идемпотентное применение патча к Flutter SDK
│   ├── check_client_identity.py  проверка идентичности по выводу собранного клиента
│   └── launch_client.py    запуск клиента с предварительной диагностикой
├── tests/                  Python-тесты (39): инструменты, подготовка клиента, launcher
└── docs/                   документация (см. README)
```

Создаётся при работе (в git не попадает): `vendor/client/` — исходный клиент,
получаемый `bootstrap.py`.

## Планируемые каталоги (не существуют)

| Каталог | Этап | Назначение |
|---|---|---|
| `client/` (расширение) | 0.2.0+ | собственные патчи и ресурсы клиента по платформам |
| `server/relay/` | 0.3.0 | сервер встреч и ретрансляции |
| `server/control/` | 0.4.0 | сервер управления, API, миграции БД |
| `panel/` | 0.6.0 | веб-панель (TypeScript/React) |
| `packaging/` | 0.3.0+ | deb, rpm, apk, PKGBUILD, Docker, systemd |
| `mobile/` | 0.7.0+ | Android и iOS |

## Как связаны части

`upstream.lock.json` → `bootstrap.py` получает исходники в `vendor/client` →
`prepare_client.py` проверяет их и применяет замены из `client/linux-identity.json` →
`client-linux.yml` собирает клиент → `check_client_identity.py` проверяет результат →
`launch_client.py` запускает его после `doctor.py`.
`crates/access-policy` пока работает отдельно и в клиент не встроен.
