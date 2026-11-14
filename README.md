# Локальный поиск

Рабочая разработческая версия приложения для Windows 11 x64: поиск имён,
содержимого документов и OCR rus+eng. Rust, Tauri 2, React/TypeScript,
SQLite FTS5, отдельные Python-процессы для PDF/Office/Tesseract.

**Это не готовый производственный релиз.** Ядро и обработчики проверены в Linux,
Windows x64 .exe и испытательный NSIS собраны перекрёстно в Linux. Windows GUI, установщик,
подпись и автономная поставка компонентов пока не проверены. Подписанного
установщика в этой поставке нет. Веб-просмотр не заменяет настольное приложение.

## Что реализовано

- Все доступные локальные фиксированные/съёмные диски по кнопке первого запуска;
  отдельные папки, исключения, сохранение настроек.
- Подстроки имён/путей, точные имена первыми, фильтры папки/расширения/дат/размера.
- PDF, DOCX, XLSX, PPTX, TXT, Markdown, CSV; фразы в кавычках, контекст,
  страницы PDF и адреса ячеек; формулы и сохранённые результаты различаются.
- OCR PNG/JPEG/TIFF/BMP и отдельных сканированных страниц смешанного PDF.
- Кэш, контроль изменений файла во время чтения, состояния очереди,
  восстановление незавершённых задач, просмотр ошибок и повтор.
- Системные уведомления и периодическая сверка, пауза/продолжение/остановка.
- Безопасный текстовый просмотр, открытие в приложении по умолчанию,
  выделение в Explorer, копирование пути, две темы и клавиатурная навигация.

Индексирование работает без искусственной задержки между файлами,
с самым низким фоновым приоритетом. Число обработчиков автоматически выбирается при запуске по числу ядер и
доступной памяти (резерв 1 ГиБ, бюджет 1,5 ГиБ на процесс); OCR использует один поток на процесс. UI и поиск не понижаются.
Для сохранения отзывчивости действуют лимиты памяти, размера и времени задач.

## Windows: подготовка и запуск из исходников

1. Установить Rust stable MSVC и Visual Studio Build Tools с C++ workload,
   Node.js 22+ и Python 3.12. Для WebView нужен Microsoft Edge WebView2 Runtime.
   Эти инструменты нужны для сборки; обычный запуск приложения не требует UAC.
2. В каталоге проекта выполнить:

```powershell
python -m venv .venv
.venv\Scripts\python.exe -m pip install -r workers\requirements.txt
npm ci
```

3. Для OCR установить Tesseract 5.x или использовать доверенную portable-сборку.
   В настройках указать путь к `tesseract.exe` и папку `tessdata` с
   `rus.traineddata`, `eng.traineddata`, `osd.traineddata`. Без OCR-компонентов
   поиск имён и текстовых документов остаётся доступен.
4. Запустить:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\windows-dev.ps1
```

Первый запуск: **Индексировать все диски** либо **Выбрать отдельные папки**.
Поиск доступен по мере появления записей. Приложение не скачивает документы,
модели или языковые файлы автоматически. Сеть не нужна после установки компонентов.

`LOCALFIND_PYTHON` задаёт путь к интерпретатору worker. Без переменной используется
`python` в Windows и `python3` в Linux. Настройки и индекс находятся в
пользовательском LocalAppData для `local.localfind.desktop`.

## Сборка установщика — пока без гарантии запуска

```powershell
powershell -ExecutionPolicy Bypass -File scripts\windows-build.ps1
```

Tauri создаёт NSIS в `target/release/bundle/nsis`. Установка настроена для
текущего пользователя. Этот установщик **не включает Python, Tesseract и модели**;
перед запуском установленной копии нужно настроить `LOCALFIND_PYTHON` и OCR.
Нет сертификата и цифровой подписи. Перед распространением необходимо выполнить
[проверки Windows](docs/WINDOWS-ACCEPTANCE.md) и закрыть лицензионный аудит поставки.
`.github/workflows/windows.yml` запускается только вручную и ничего не публикует.

## Проверки ядра и интерфейса

```sh
cargo test -p localfind-core --locked
cargo clippy -p localfind-core --all-targets -- -D warnings
python -m pip install -r workers/requirements-dev.txt
python -m unittest discover -s tests -v
npm ci
npm run build
npm run test:ui
cargo run -p localfind-core --release --example benchmark
cargo run -p localfind-core --release --example filesystem_benchmark
```

Для OCR-части теста смешанного PDF задать `LOCALFIND_TEST_TESSDATA`.
Playwright по умолчанию использует `/usr/bin/chromium`; на Windows задать
`PLAYWRIGHT_CHROMIUM_EXECUTABLE` или установить Chromium через `npx playwright install chromium`.
UI-тест IPC использует контрактный двойник, а не настоящий Windows backend;
реальная связка Rust → Python → SQLite проверяется интеграционными тестами ядра.

CLI для воспроизводимой проверки реального индекса:

```sh
cargo run -p localfind-core -- ./local-data/index.sqlite add ./fixtures
cargo run -p localfind-core -- ./local-data/index.sqlite scan
cargo run -p localfind-core -- ./local-data/index.sqlite process
cargo run -p localfind-core -- ./local-data/index.sqlite search '"договор поставки"'
```

Блокировка файла запрещает параллельное открытие одной БД разными экземплярами
CLI/GUI: это защищает восстановление состояний очереди.

## Документация

- [Согласованный объём](docs/SCOPE.md)
- [Архитектура и решения](docs/ARCHITECTURE.md)
- [Поддержанные форматы и ограничения](docs/FORMATS.md)
- [Результаты тестов и измерений](docs/VALIDATION.md)
- [OCR: сравнение и выбор](docs/OCR.md)
- [Сторонние лицензии](docs/THIRD-PARTY.md)

В репозитории нет пользовательских документов, моделей, индексов, секретов,
готовых зависимостей и публикации в удалённый репозиторий.
