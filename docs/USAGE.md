# Как пользоваться

Автор: SecretHero. Разделы «Сейчас» работают на версии 0.1.3; разделы «По плану»
пока не реализованы.

## Сейчас: диагностика Linux

```bash
python3 tools/doctor.py          # текстовый отчёт
python3 tools/doctor.py --json   # JSON
```

Запускайте от обычного пользователя в графическом сеансе. Код выхода: 0 — хорошо,
1 — предупреждение, 2 — проблема. Если в X11 нет `libxdo`, мышь в удалённом
сеансе может не работать при работающей клавиатуре. В Simply/ALT:

```bash
su -
apt-get install xdotool
```

Затем полностью перезапустите клиент. Это прервёт текущий удалённый сеанс.

## Сейчас: проверка кода

```bash
python3 -m unittest discover -s tests -v
python3 tools/bootstrap.py --check-lock
cargo test --workspace --offline
```

## Сейчас: получить исходники клиента и собрать (для разработчиков)

```bash
python3 tools/bootstrap.py --submodules                 # в vendor/client
python3 tools/prepare_client.py vendor/client --report dist/report.json --profile zherodizk
```

`bootstrap.py` создаёт только новый каталог и ничего не перезаписывает.
`prepare_client.py` отказывается работать с изменённым checkout и при сбое записи
возвращает файлы в прежнее состояние. Полная сборка выполняется в GitHub Actions:
Actions → «Linux client baseline» → Run workflow. Подробности —
[CLIENT-LINUX.md](CLIENT-LINUX.md).

## Сейчас: запуск собранного клиента

Из каталога собранного пакета, обычным пользователем:

```bash
python3 zherodizk.py --doctor    # только диагностика
python3 zherodizk.py             # диагностика, затем запуск
```

Это экспериментальная сборка для проверки. Исполняемые файлы публично не
распространяются, настоящих правил доступа в ней нет.

## Сейчас: сервер управления (для разработчиков и испытаний)

Это готовый к испытаниям, но не упакованный сервер: без TLS (ставьте за обратный прокси) и без systemd-юнита.
Нужен PostgreSQL 16.

```bash
cargo build --release -p zherodizk-control
export ZHD_DATABASE_URL=postgres://zhd:пароль@localhost/zhd
export ZHD_LISTEN=127.0.0.1:21114          # по умолчанию
./target/release/zherodizk-control generate-keys          # печатает ZHD_MFA_KEY и ZHD_GRANT_KEY
export ZHD_MFA_KEY=...        # включает MFA; хранить в секрете, потеря делает секреты MFA непригодными
export ZHD_GRANT_KEY=...      # ключ подписи разрешений на сеанс; хранить в секрете
./target/release/zherodizk-control
```

Первый аккаунт (администратор) создаётся запросом `POST /v1/auth/register`; дальше регистрация закрыта,
пока не задано `ZHD_ALLOW_REGISTRATION=true`. Описание всех запросов — [api/openapi.yaml](api/openapi.yaml).
Потеря `ZHD_MFA_KEY` делает сохранённые секреты MFA непригодными.

## По плану: пользователь

1. Установить клиент для своей платформы (0.2.0 и далее).
2. Указать адрес своего сервера или получить его у администратора.
3. Передать другому ID и разрешение либо подключиться к устройству из адресной книги.

## По плану: администратор

1. Установить сервер из пакета или Docker Compose ([SERVER.md](SERVER.md)).
2. Создать организацию и администратора, включить MFA.
3. Зарегистрировать устройства, настроить группы и права в панели.
4. Просматривать журнал и завершать активные сеансы.
