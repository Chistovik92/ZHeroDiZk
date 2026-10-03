# ZHeroDiZk — состояние на 4 октября 2026

Автор: SecretHero.

Версия проекта: 0.7.7 (пре-релиз; веб-панель, сборка Android, установщики в выпуске, руководство). Это начало разработки, не законченный продукт; этап агента не начат, панель и Android не проверены на живых системах.
Дорожная карта и версии: [ROADMAP.md](ROADMAP.md).
PR: https://github.com/Chistovik92/ZHeroDiZk/pull/2.

## Проверки 0.7.7

Ветка `claude/github-sync-release-0-7-7-00c2a1`. Все удалённые ветки репозитория уже входили в `main` (слияний не потребовалось).

- **Сервер управления:** 3 новых запроса (`GET …/members`, `GET …/grants`, `POST /v1/auth/password`), интеграционные тесты на PostgreSQL 16 и clippy без замечаний — [https://github.com/Chistovik92/ZHeroDiZk/actions/runs/37157786594](https://github.com/Chistovik92/ZHeroDiZk/actions/runs/37157786594). Описание OpenAPI сверяется с маршрутами тестом.
- **Веб-панель** (`panel/`): проверка типов, сборка Vite, 6 тестов vitest, `npm audit` без уязвимостей, архив — [https://github.com/Chistovik92/ZHeroDiZk/actions/runs/37157784725](https://github.com/Chistovik92/ZHeroDiZk/actions/runs/37157784725). Внешний вид просмотрен в браузере только на заглушке API; прохода по живому серверу не было.
- **Android:** сборка, подпись и проверка трёх APK (arm64-v8a, armeabi-v7a, x86_64) — [https://github.com/Chistovik92/ZHeroDiZk/actions/runs/37157784789](https://github.com/Chistovik92/ZHeroDiZk/actions/runs/37157784789). `apksigner verify` проходит; `aapt2`: пакет `io.github.chistovik92.zherodizk`, название ZHeroDiZk, `versionName` 0.7.7; `check_client_network.py` не нашёл чужих адресов и ключей в `librustdesk.so`, `libapp.so`, `libflutter.so`. В этом запуске использован временный тестовый ключ (секретов репозитория ещё нет). **На устройстве приложение не запускалось.**
- Python-тесты (93) и проверка lock — [https://github.com/Chistovik92/ZHeroDiZk/actions/runs/37157784682](https://github.com/Chistovik92/ZHeroDiZk/actions/runs/37157784682).
- Ключ подписи Android создан автором проекта (RSA 4096, срок до 2056 года), копия вне репозитория. Отпечаток сертификата SHA-256: `B1:6F:0C:F1:D8:ED:08:7B:52:5A:FB:25:27:11:B9:41:CC:98:0C:1C:2D:49:51:D5:B7:C6:08:CE:78:FD:0D:39`.

Выпуск v0.7.7 (https://github.com/Chistovik92/ZHeroDiZk/releases/tag/v0.7.7) собран workflow `release.yml` ([https://github.com/Chistovik92/ZHeroDiZk/actions/runs/37159782186](https://github.com/Chistovik92/ZHeroDiZk/actions/runs/37159782186)): 20 файлов и `SHA256SUMS`; APK подписаны временным ключом `-citestkey` (секреты постоянного ключа ещё не установлены).

Не проверялось: работа панели с живым сервером в браузере, установка и запуск APK, пакетов и клиентов на реальных устройствах.

## Проверки 0.7.6 — сбор лицензий

Код: `a412895fc8000fbdedb69ea89bbaa514034eb645`.
Локально прошли 92 Python-теста, `compileall`, проверка lock-файла и `git diff --check`.
В CI прошли [source-checks](https://github.com/Chistovik92/ZHeroDiZk/actions/runs/37154131369),
[Rust-тесты с PostgreSQL и Clippy](https://github.com/Chistovik92/ZHeroDiZk/actions/runs/37154131359),
[полная Linux-сборка](https://github.com/Chistovik92/ZHeroDiZk/actions/runs/37154131560) и
[полная Windows-сборка](https://github.com/Chistovik92/ZHeroDiZk/actions/runs/37154133340).

Скачанные SBOM проверены отдельно: Linux — 982 компонента (753 Cargo, 214 Dart, 15 vcpkg),
Windows — 808 (578 Cargo, 214 Dart, 16 vcpkg). В каждом отчёте тексты лицензий есть у 211 Dart-пакетов
и всех пакетов vcpkg; 12 компонентов остаются UNKNOWN (9 Rust и 3 пакета Flutter SDK).
Пункт 0.1.7 закрыт как сбор лицензионных данных; юридическая проверка неизвестных лицензий не завершена.
Сборки проверяют бинарники и отчёты, но не подтверждают работу удалённых сеансов на реальных устройствах.
Выпуск содержит исходники и SBOM, без новых установщиков.

## Подтверждённая сборка Linux-клиента

Коммит: `1fc6aca249bd15323e265dca765b469b4f9a8d4c`.
Запуск: https://github.com/Chistovik92/ZHeroDiZk/actions/runs/34693442022.
Результат: **success**. На Ubuntu 22.04 x86_64 выполнены:

- Получение закреплённого исходного клиента с подмодулями и проверка исходников.
- Генерация Dart/Rust bridge и сборка кодеков через vcpkg.
- Компиляция Rust-библиотеки и Flutter-интерфейса.
- Проверка наличия бинарников, динамических библиотек и команды `--version`.
- Запуск диагностического launcher в Xvfb/X11.

Архив отчёта скачан и проверен отдельно: версия клиента `1.4.8`,
в двух отчётах ldd нет `not found`, диагностика libxdo проходит.
SHA-256 архива: `907904c0906a8a38bb947b10dd534972ed34f05a263df4446b00f555598fd597`.
Артефакт: https://github.com/Chistovik92/ZHeroDiZk/actions/runs/34693442022/artifacts/10298134110.
Архив хранится в Actions 14 дней; это только отчёты, без исполняемых файлов.

SHA-256 полученных бинарников:
- исходный бинарник клиента: `3f8f793bdd27f0674a0bd698d41bc7a1c2835085552e21f2200ad920e8f84306`.
- librustdesk.so: `6041d71168c35bedede7cc423398ba6d03cd3b89b06834022794ab852a6cdc3c`.

## Проверки исходников

На том же коммите прошли 27 Python-тестов и 17 Rust-тестов:
https://github.com/Chistovik92/ZHeroDiZk/actions/runs/34693444030.

После успешной сборки добавлена проверка повторного применения патча к
кешированному Flutter SDK. Локально проходят 29 Python-тестов, включая
реальное применение Git-патча дважды и отказ при конфликте без перезаписи.
Результат актуальной ветки смотрите в проверках PR.

Последующие изменения workflow: идемпотентное применение SDK-патча,
проверка точного вывода версии (upstream runner может вернуть ноль при ошибке
загрузки библиотеки) и ручной запуск тяжёлой сборки. Полная сборка выше
относится именно к указанному коммиту; после этих изменений она не повторялась.

## Что изменено

Диагностика проверяет libxdo.so.4, libxdo.so.3, libxdo.so в порядке upstream.
Повреждённая первая загружаемая библиотека не маскируется последующей;
libxdot.so.4 не принимается за libxdo. Launcher останавливает запуск при ошибке.
Подготовка клиента проверяет SHA, чистоту checkout и состояние подмодулей.

Устранены две найденные сборкой ошибки: конфликт зависимости libunwind-dev
и недостаточная история Git для закреплённых портов vcpkg.
Новый код сохраняет AGPL-3.0-only; вопрос лицензии hbb_common — в LICENSING.md.

## Упаковка сервера управления, ограничение частоты, значок (3 октября 2026)

- Ограничение частоты анонимных запросов проверено тестами на PostgreSQL (https://github.com/Chistovik92/ZHeroDiZk/actions/runs/37085451401).
- Пакеты `zherodizk-control` (deb, rpm, tarball; x86_64 и aarch64), юнит systemd, Docker Compose с PostgreSQL: сборка, установка на 8 дистрибутивах, запуск службы с настоящим PostgreSQL на раннере со systemd и через Compose проверены в CI (https://github.com/Chistovik92/ZHeroDiZk/actions/runs/37092687297).
- Клиенты Linux (https://github.com/Chistovik92/ZHeroDiZk/actions/runs/37086311425) и Windows (https://github.com/Chistovik92/ZHeroDiZk/actions/runs/37086313897) собираются со значком и строками ZHeroDiZk; в бинарниках нет чужих адресов. Внешний вид в работе не проверялся (клиент не запускался).

## Разрешения на сеанс (3 октября 2026)

`crates/grant`: 13 тестов; сервер: 7 интеграционных тестов выдачи и отзыва на PostgreSQL (https://github.com/Chistovik92/ZHeroDiZk/actions/runs/37084964176). Проверено: выдача только по правилам доступа, проверка на стороне устройства, повтор, чужое устройство, истечение, чужой ключ, отзыв оператором и админом, запрет разрешений между организациями на уровне БД. **Не сделано:** агент не требует разрешений (клиент по-прежнему принимает прежние подключения), нет доставки списка отзыва, нет проверки на реальных устройствах.

## Сервер управления и клиенты без чужих адресов (3 октября 2026)

- `crates/control`: 30 модульных и 31 интеграционный тест на PostgreSQL 16, clippy без замечаний (https://github.com/Chistovik92/ZHeroDiZk/actions/runs/37084017476). Покрыто: аккаунты, блокировка, MFA (в т.ч. векторы RFC 4226/6238), организации, устройства, группы, ACL, адресная книга, аудит, соответствие OpenAPI маршрутам.
- Linux-клиент (https://github.com/Chistovik92/ZHeroDiZk/actions/runs/37081370611) и Windows-клиент (https://github.com/Chistovik92/ZHeroDiZk/actions/runs/37081373712) собираются; в `zherodizk`, `librustdesk.so`, `libapp.so`, `zherodizk.exe`, `librustdesk.dll`, `app.so` нет адресов `api./admin./rs-ny.rustdesk.com` и встроенного ключа. Запуск клиентов не проверялся; соединение клиент-сервер не проверялось.
- Не сделано: TLS и пакеты сервера управления, панель, управляемые сеансы, Android/macOS/iOS.

## Windows и сервер (3 октября 2026)

- Windows-клиент: сборка прошла (https://github.com/Chistovik92/ZHeroDiZk/actions/runs/37076929471); `zherodizk.exe` с метаданными ZHeroDiZk/SecretHero проверен по собранному файлу. Запуск на Windows не проверялся. Пользовательский движок Flutter скачивается как в исходной сборке без закрепления версии; его SHA-256 записан в отчёте сборки.
- Сервер: статические бинарники `zhd-rendezvous`, `zhd-relay`, `zhd-utils`; пакеты и установка проверены в CI на 8 дистрибутивах (https://github.com/Chistovik92/ZHeroDiZk/actions/runs/37079026760) и aarch64 под эмуляцией (https://github.com/Chistovik92/ZHeroDiZk/actions/runs/37080147593); Docker Compose запускается.
- Проблема со встроенными адресами исходного проекта исправлена в 0.4.0 (SECURITY.md, пункт 0.3.7).

## Сборка с собственной идентичностью

Коммит `9f8f7a1` (ветка идентичности): полная сборка Linux-клиента успешна, шаг
«Assemble and check development bundle» включает проверку идентичности по выводу
самого бинарника: https://github.com/Chistovik92/ZHeroDiZk/actions/runs/34695568062.
Повторные сборки `main` на `1280341` (v0.1.3) и `6d3da01` (v0.1.5, с отчётом SBOM) завершились
успешно: https://github.com/Chistovik92/ZHeroDiZk/actions/runs/37069883261 и
https://github.com/Chistovik92/ZHeroDiZk/actions/runs/37072336961.

## Проверки на 3 октября 2026

Локально (Windows, Python 3.13): 39 Python-тестов проходят после слияния ветки
идентичности и исправлений устойчивости (UTF-8, окончания строк, откат записи).
Rust не установлен на машине автора: Rust-тесты (17) в этом запуске **не выполнялись**
локально и проверяются в GitHub Actions. Полная сборка Linux-клиента после слияния
не запускалась.

## Что ещё не подтверждено

- Работа GUI и реальное управление мышью/клавиатурой на Simply, Windows, Android.
- Совместимость Ubuntu-бинарника с Simply/ALT и установочный пакет ALT.
- Собственный интерфейс (идентичность Linux уже проверена сборкой в CI, графика и тексты — исходные).
- Интеграция policy, токены, API, БД, панель, hbbs/hbbr, обновления.

Сборка пока использует исходные интерфейс и сетевое поведение; в Linux-профиле заменены имя, ID приложения и каталоги.
Наш модуль правил доступа не защищает его сеансы: интеграции пока нет.
Готовых установщиков ZHeroDiZk и публичного релиза продукта нет.
