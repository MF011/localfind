# Сторонние компоненты

Эта поставка содержит исходники проекта и lock-файлы, без готовых сторонних
runtime/model-бинарников. Перед распространением установщика необходимо собрать
license notices **всех** фактически упакованных транзитивных компонентов.
Текущий список — информация об основных компонентах, не завершённый юридический аудит.

| Компонент | Лицензия / особенности |
|---|---|
| Rust std/toolchain | MIT / Apache-2.0; toolchain не поставляется пользователю |
| Tauri 2, tauri-build, официальные плагины | MIT OR Apache-2.0 |
| React, React DOM, Vite, open | MIT |
| TypeScript | Apache-2.0 |
| Lucide | ISC |
| SQLite | Public domain; bundled через rusqlite |
| rusqlite | MIT |
| notify | CC0-1.0 |
| walkdir | Unlicense OR MIT |
| serde, serde_json, anyhow | MIT OR Apache-2.0 |
| BLAKE3 | CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception |
| Python | PSF-2.0 и notices стандартной библиотеки |
| pypdfium2 5.13.0 | BSD-3-Clause / Apache-2.0; включает отдельные notices PDFium |
| PDFium | BSD-style + лицензии зависимостей; использовать bundled notices дистрибутива |
| Pillow 13.0.0 | MIT-CMU + notices включённых библиотек изображений |
| Tesseract | Apache-2.0; Leptonica и кодеки имеют отдельные лицензии |
| tessdata_fast rus/eng/osd | Apache-2.0; сохранять upstream notices при поставке |
| WebView2 | Microsoft runtime terms, не MIT; способ распространения проверять отдельно |
| NSIS | zlib/libpng; лицензии bundled plugins проверять отдельно |
| PaddleOCR/PaddlePaddle | Apache-2.0; только эксперимент, в приложение не включены |
| ReportLab | BSD; только генерация тестовых PDF |
| Playwright | Apache-2.0; только тесты |
| Prettier | MIT; только инструменты разработки |

Версии Rust и JS зависимостей закреплены Cargo.lock/package-lock.json.
Python runtime-зависимости закреплены workers/requirements.txt.
`results/dependency-inventory.json` содержит инвентаризацию lock-файлов; для
неполученных в Linux target-specific crate лицензия отмечена как неизвестная.
Неизвестные лицензии требуется проверить перед Windows binary redistribution.
Нет сертификата подписи. Windows installer ещё не валидирован и не опубликован.
