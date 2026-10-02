# ZHeroDiZk

[![Проверки исходников](https://github.com/Chistovik92/ZHeroDiZk/actions/workflows/checks.yml/badge.svg)](https://github.com/Chistovik92/ZHeroDiZk/actions/workflows/checks.yml)

**Версия: 0.1.5 (пре-релиз) · Автор: SecretHero ([Telegram @SecretHero](https://t.me/SecretHero)) · Лицензия: AGPL-3.0-only**

ZHeroDiZk — открытый проект удалённого доступа в классе RustDesk и AnyDesk:
собственный сервер на Linux и клиенты для Windows, Linux, Android, macOS и iOS,
с управляемыми правами доступа, администраторской панелью и журналом аудита.

> **Честный статус.** Проект на ранней стадии. Готового продукта, установщиков,
> APK, развёрнутого сервера и панели **пока нет**. Сейчас есть: инженерные
> инструменты, библиотека правил доступа и воспроизводимая сборка Linux-клиента
> в GitHub Actions. Подробно — [docs/STATUS.md](docs/STATUS.md).
>
> **Безопасность.** Клиент в текущей сборке по умолчанию обращается к публичному серверу
> исходного проекта (встроенный адрес и ключ). Бинарники клиентов не публикуются, пока это не
> исправлено (дорожная карта, 0.3.7). Серверные бинарники ZHeroDiZk такого поведения лишены.

## Цели

- Серверы на Linux (Debian/Ubuntu, ALT, семейство RHEL, Alpine, Arch): бинарник,
  пакеты, Docker Compose.
- Клиенты: Windows, Linux, Android, macOS, iOS (в пределах возможностей платформ,
  см. [docs/PLATFORMS.md](docs/PLATFORMS.md)).
- Своя идентичность во всех слоях и свой интерфейс.
- Управляемые права: разрешения на сеанс, согласие пользователя, отзыв, аудит.
- Бесплатно и открыто: без платных редакций и проверки лицензионного сервера.
  Серверы и трафик всё равно стоят денег — это не скрывается.

## Что реализовано и что запланировано

| Область | Состояние |
|---|---|
| Название, лицензия, CI исходных проверок | ✅ |
| Диагностика Linux/X11 (`tools/doctor.py`, libxdo) | ✅ |
| Получение и проверка закреплённых исходников клиента | ✅ |
| Аудит лицензии `hbb_common` ([docs/LICENSE-AUDIT.md](docs/LICENSE-AUDIT.md)) | ✅ выполнен; подтверждение правообладателя ⬜ |
| SBOM и отчёт о лицензиях (`tools/sbom.py`) | ✅ v0.1.5; лицензии Dart ещё не собираются (0.1.7) |
| Библиотека правил доступа `access-policy` (Rust, 17 тестов) | ✅ как модуль; ⬜ не встроена в клиент |
| Сборка Linux-клиента в Actions | ✅ (запуск на `1fc6aca`) |
| Собственная идентичность Linux-клиента | ✅ сборка на `9f8f7a1` прошла в CI (проверка по бинарнику) |
| Клиент Windows | 🟡 0.2.0: сборка и идентичность проверяются в CI |
| Сервер встреч и ретрансляции (`zhd-rendezvous`, `zhd-relay`) | 🟡 собирается, устанавливается из пакетов (8 дистрибутивов), Docker; готовых пакетов для скачивания нет — [docs/SERVER.md](docs/SERVER.md) |
| Сервер управления, аккаунты, MFA, PostgreSQL | ⬜ 0.4.0 |
| Управляемые сеансы | ⬜ 0.5.0 |
| Веб-панель | ⬜ 0.6.0 |
| Android / macOS / iOS | ⬜ 0.7.0 / 0.8.0 / 0.9.0 |
| Публичный релиз | ⬜ 1.0.0 |

Полная дорожная карта с версиями и ориентировочными сроками —
[docs/ROADMAP.md](docs/ROADMAP.md).

## Версии

`0.X.0` — большие этапы, `0.X.Y` — их подпункты, `X.0.0` — релизы
(`1.0.0` — первый публичный). См. [docs/VERSIONING.md](docs/VERSIONING.md).

## Быстрый старт для разработчика

Нужны Git, Python 3.10+; для Rust-модуля — Rust 1.75+.

```bash
git clone https://github.com/Chistovik92/ZHeroDiZk.git
cd ZHeroDiZk
python3 -m unittest discover -s tests -v
python3 tools/bootstrap.py --check-lock
cargo test --workspace --offline
```

Диагностика Linux-компьютера (читает окружение, ничего не меняет):

```bash
python3 tools/doctor.py
python3 tools/doctor.py --json
```

Коды выхода: 0 — всё хорошо; 1 — предупреждение или неполная проверка;
2 — проблема. Диагностика не проверяет захват экрана, разрешения и реальные клики.

Сборка Linux-клиента — [docs/CLIENT-LINUX.md](docs/CLIENT-LINUX.md).
Все сценарии использования — [docs/USAGE.md](docs/USAGE.md).

## Документация

| Документ | О чём |
|---|---|
| [docs/ROADMAP.md](docs/ROADMAP.md) | этапы, версии, статусы, сроки |
| [docs/STATUS.md](docs/STATUS.md) | что подтверждено проверками и что нет |
| [docs/VERSIONING.md](docs/VERSIONING.md) | правила версий и выпуска |
| [docs/PROJECT-STRUCTURE.md](docs/PROJECT-STRUCTURE.md) | структура репозитория |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | компоненты и модель доступа |
| [docs/PLATFORMS.md](docs/PLATFORMS.md) | матрица платформ и ограничения |
| [docs/SERVER.md](docs/SERVER.md) | сервер: планируемая установка и эксплуатация |
| [docs/USAGE.md](docs/USAGE.md) | как пользоваться: сейчас и по плану |
| [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) | разработка, проверки, правила вклада |
| [docs/CLIENT-LINUX.md](docs/CLIENT-LINUX.md) | сборка и запуск Linux-клиента |
| [docs/SECURITY.md](docs/SECURITY.md) | модель угроз и меры |
| [docs/LICENSING.md](docs/LICENSING.md) | лицензии и их обязательства |
| [docs/LICENSE-AUDIT.md](docs/LICENSE-AUDIT.md) | аудит лицензии подмодуля |
| [docs/NAME-CHECK.md](docs/NAME-CHECK.md) | проверка названия |
| [docs/HANDOFF.md](docs/HANDOFF.md) | заметки для продолжения работы |
| [CHANGELOG.md](CHANGELOG.md) | история изменений |

## Лицензия и авторство

Автор проекта — **SecretHero** (Telegram: [@SecretHero](https://t.me/SecretHero)). Новые исходники: AGPL-3.0-only, полный текст в
[LICENSE](LICENSE). Проект использует сторонний открытый код под своими
лицензиями; обязательные уведомления сохранены в [NOTICE](NOTICE) и
[docs/LICENSING.md](docs/LICENSING.md) и не удаляются.
