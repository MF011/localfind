# Выбор OCR

Выбран **Tesseract 5.5.0**, rus+eng из tessdata_fast, OSD, autocontrast и sharpening.
Причины: на нашем небольшом контрольном наборе заметно выше точность, проще
локальная CPU-поставка, нет тяжёлого Python inference runtime, Apache-2.0.

Сравнение выполнено 2026-11-14 на Linux x64, Xeon Platinum 8370C (3 vCPU),
около 9,7 ГиБ доступной RAM машины. Четыре синтетические картинки DejaVu Sans:
русский, английский, смешанный текст с размытием/наклоном и русский с поворотом 90°.
Приватные данные не использовались. Это не репрезентативный промышленный корпус
фотографий, рукописей и сложных вёрсток; общий вывод о превосходстве движка недопустим.

Paddle: PaddleOCR 2.10.0, PaddlePaddle 3.0.0 CPU, PP-OCRv3 multilingual detector,
cyrillic recognizer, angle classifier; 1 CPU thread, MKL-DNN выключен. Это конкретная
воспроизводимая конфигурация, а не оценка всех современных моделей PaddleOCR.
Tesseract включает ориентацию OSD и предобработку; Paddle использует свой pipeline.
Время не включает установку/загрузку моделей; первый вызов может быть холодным.

| Образец | Tesseract CER | Tesseract, с | Paddle CER | Paddle, с |
|---|---:|---:|---:|---:|
| Русский | 1,69% | 1,281 | 17,80% | 1,448 |
| Английский | 0,00% | 1,165 | 11,97% | 0,562 |
| Смешанный, размытие | 1,83% | 1,339 | 38,53% | 1,025 |
| Поворот 90° | 1,69% | 1,300 | 67,80% | 0,546 |

CER — расстояние Левенштейна / длина эталона после casefold и нормализации пробелов.
Средний CER (по образцам): 1,30% против 34,03%. Суммарно 5,085 с против 3,581 с.
Ошибки порядка строк также учитываются. Tesseract путает латинские AB с кириллическими
АВ в русском контексте — это влияет на поиск кодов и явно не считается решённым.
Сырые ответы: `results/tesseract.json`, `results/paddle.json`.

Повтор:

```sh
# Отдельное окружение; Paddle нужен только для эксперимента, не для приложения.
python -m pip install paddleocr==2.10.0 paddlepaddle==3.0.0
python scripts/compare_ocr.py --work /absolute/scratch/ocr --tessdata /absolute/tessdata --engine tesseract
python scripts/compare_ocr.py --work /absolute/scratch/ocr --tessdata /absolute/tessdata --engine paddle
```

Скрипт сравнения Paddle автоматически получает около 15,3 МиБ моделей
(detector ~3,7 МиБ, recognizer ~9,5 МиБ, orientation ~2,1 МиБ). CPU-wheel Paddle
3.0.0 — около 184 МиБ, дополнительные зависимости требуют ещё несколько сотен МБ.
Модели сохраняются под `--work`, не в репозитории. Это поведение экспериментального
скрипта; само приложение ничего не скачивает.

Tesseract-модели использованного теста (SHA-256):

- rus: `e16e5e036cce1d9ec2b00063cf8b54472625b9e14d893a169e2b0dedeb4df225`
- eng: `7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2`
- osd: `9cf5d576fcc47564f11265841e5ca839001e7e6f38ff7f7aacf46d15a96b00ff`

Источник rus: https://github.com/tesseract-ocr/tessdata_fast (около 3,7 МиБ).
eng и osd установлены в тестовой Debian-среде. Модели не включены в исходники.
