# Сервер ZHeroDiZk

Автор: SecretHero. Состояние: **сервер встреч и ретрансляции собирается и устанавливается из
пакетов (проверено в CI); сервер управления, аккаунты, панель — ещё не начаты** (этапы 0.4.0 и 0.6.0).

> **Важно.** Готовых пакетов для скачивания пока нет: бинарники намеренно не публикуются
> (см. [SECURITY.md](SECURITY.md), условия релиза). Пакеты собираются workflow
> `Linux server` в GitHub Actions; в вывод сохраняются только отчёты и контрольные суммы.
> Клиент ZHeroDiZk в текущей сборке по умолчанию обращается к публичному серверу исходного
> проекта — до замены этого поведения (этап 0.3.x) вручную укажите свой сервер.

## Состав

| Программа | Назначение | Порты |
|---|---|---|
| `zhd-rendezvous` | сервер встреч: регистрация устройств, помощь в прямом соединении | 21115/tcp (проверка NAT), 21116/tcp и 21116/udp, 21118/tcp (websocket) |
| `zhd-relay` | ретранслятор зашифрованного потока | 21117/tcp, 21119/tcp (websocket) |
| `zhd-utils` | генерация и проверка пары ключей, диагностика связи | — |

Ретранслятор видит метаданные соединений, но не ключи содержимого.
Автоматическая проверка обновлений на сторонних серверах в нашей сборке **отключена**.

## Что проверено и на чём

Проверки выполняются в CI (workflow `Linux server`, запуск 37079026760 и последующие):

| Дистрибутив | Способ | Результат |
|---|---|---|
| Debian 12 | `.deb` | ✅ установка, пользователь, запуск `zhd-utils` |
| Ubuntu 22.04, 24.04 | `.deb` | ✅ |
| Rocky Linux 9, AlmaLinux 9, Fedora 41 | `.rpm` | ✅ |
| ALT p10, ALT p11 | `.rpm` | ✅ (минимальные образы без systemd) |
| Alpine 3.20 | tarball | ✅ запуск `zhd-utils` |
| Ubuntu (раннер GitHub) | `.deb` + systemd | ✅ запуск обеих служб, порты, права |
| Docker / Compose | образ на Alpine | ✅ запуск, порты |
| Arch Linux | PKGBUILD (`packaging/arch`) | ✅ сборка `makepkg` и установка (sysusers/tmpfiles) |
| aarch64 | `.deb`, `.rpm`, tarball | ✅ установка под эмуляцией (Debian 12, Fedora 41, Alpine); на реальном железе не запускалось |

В контейнерах без systemd проверяются установка и пользователь, а не запуск службы.

## Установка из пакета (после появления пакетов)

Debian / Ubuntu:

```bash
sudo apt install ./zherodizk-server_<версия>_amd64.deb
```

RHEL-семейство, Fedora, ALT:

```bash
sudo dnf install ./zherodizk-server-<версия>-1.x86_64.rpm      # RHEL, Fedora
sudo rpm -i zherodizk-server-<версия>-1.x86_64.rpm             # ALT
```

Остальные дистрибутивы: распакуйте `zherodizk-server-<версия>-linux-<arch>.tar.gz`
(статические бинарники, зависимостей нет) и запустите `zhd-rendezvous` и `zhd-relay`.

Пакет создаёт системного пользователя `zherodizk`, каталог состояния `/var/lib/zherodizk`
(права 0750), файл настроек `/etc/zherodizk/server.env` и юниты systemd. Службы
**не включаются автоматически**.

## Настройка и запуск

1. Откройте `/etc/zherodizk/server.env`. Параметры по умолчанию (`-k _`) разрешают
   подключение только клиентам с открытым ключом сервера. Для указания адреса ретранслятора
   добавьте к `ZHD_RENDEZVOUS_ARGS`, например: `-r relay.example.org -k _`.
2. Включите службы:

```bash
sudo systemctl enable --now zhd-rendezvous zhd-relay
systemctl status zhd-rendezvous zhd-relay
```

3. Ключи создаются при первом запуске в `/var/lib/zherodizk`: закрытый `id_ed25519`
   (права 0600, хранить в секрете и включать в резервные копии) и открытый
   `id_ed25519.pub` — его нужно указать в клиентах.
4. Откройте порты в брандмауэре (таблица выше), в том числе **udp/21116**.

## Docker Compose

```bash
docker build -f packaging/docker/Dockerfile --build-arg BIN_DIR=bin/amd64 -t zherodizk-server:local .
docker compose -f packaging/docker/docker-compose.yml up -d
```

В каталоге `bin/amd64` должны лежать `zhd-rendezvous`, `zhd-relay`, `zhd-utils`. Контейнеры
работают без привилегий, с read-only файловой системой; ключи хранятся в томе `zhd-data`.

## Сборка пакетов самостоятельно

```bash
python3 tools/bootstrap.py --component server --submodules
python3 tools/prepare_server.py vendor/server --report dist/source.json
cd vendor/server && cross build --release --all-features --target x86_64-unknown-linux-musl
sh packaging/build-deb.sh <каталог-с-бинарниками> <версия> amd64 <выход>
sh packaging/build-rpm.sh <каталог-с-бинарниками> <версия> x86_64 <выход>
```

Нужны Rust 1.90, `cross`, Docker; для rpm — `rpmbuild`. Воспроизводимая цепочка — workflow
`.github/workflows/server-linux.yml`.

## Обновление и удаление

Установите новый пакет поверх старого: по настройкам пакета `server.env` не перезаписывается, каталог
`/var/lib/zherodizk` и ключи сохраняются (обновление поверх старой версии отдельным тестом пока не проверялось). Удаление пакета останавливает и отключает службы;
данные и пользователь остаются (удаляйте вручную, если ключи больше не нужны).

## Сервер управления

Исходники и тесты — `crates/control`, интерфейс — [api/openapi.yaml](api/openapi.yaml), запуск для испытаний —
[USAGE.md](USAGE.md). Переменные окружения: `ZHD_DATABASE_URL` (обязательно), `ZHD_LISTEN` (по умолчанию
`127.0.0.1:21114`), `ZHD_ALLOW_REGISTRATION` (`true`/`false`), `ZHD_MFA_KEY` (32 байта в base64; без него MFA отвечает 503).
Пакеты, systemd-юнит и Docker-образ для него пока не сделаны, TLS не реализован (нужен обратный прокси).

## Чего ещё нет

Упаковка сервера управления и TLS; панель — 0.6.0; метрики и
нагрузочные испытания — 0.3.6; подписанные пакеты и репозиторий пакетов; пакет apk для Alpine.
Стоимость серверов и трафика ложится на того, кто разворачивает сервер.
