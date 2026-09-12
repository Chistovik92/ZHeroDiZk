# ZHeroDiZk — состояние на 12 сентября 2026

Версия: 0.1.0-dev.1. Начало разработки, не законченный аналог RustDesk.
Репозиторий: https://github.com/Chistovik92/ZHeroDiZk.

## Подтверждённая проверка исходной основы

Коммит: 9b4e2ac49a8df1ec2570088d752c7efa7f495991.
GitHub Actions: https://github.com/Chistovik92/ZHeroDiZk/actions/runs/34692448902.
Результат: success; шаги Python integration tests, Lock validation и
Rust policy tests завершились успешно. Проверены 16 Python-тестов и 17 Rust-тестов.

Проверки охватывают:
- Загрузку именно libxdo.so.3 и наличие функций мыши.
- CLI JSON, коды выхода и изоляцию нативной загрузки.
- Реальный локальный Git checkout по SHA и запрет перезаписи существующей папки.
- Правила доступа: организации, привязки, время, отзыв, согласие и capabilities.

Успех этого запуска относится к указанному коммиту, не к будущим изменениям.
Актуальный результат ветки смотрите в GitHub Actions/проверках Pull Request.

## Текущие изменения

Принято имя ZHeroDiZk, обновлены brand.json, NOTICE, README и handoff.
Имя Rust-пакета — zherodizk-access-policy. Логика правил не изменена.
История отклонённого имени ZeroDisk сохранена в NAME-CHECK.md.

## Не завершено

- Сборка/запуск форка RustDesk и реальный GUI-тест на Windows/Simply.
- Сетевая загрузка upstream через bootstrap с подмодулями: код есть, не проверена.
- Интеграция policy в агент, токены, API, БД, панель, hbbs/hbbr, обновления.

Действующий RustDesk не защищается нашим policy crate: интеграции пока нет.
Готовых EXE/APK и публичного релиза продукта нет.
