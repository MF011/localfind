"""One bounded, isolated extraction request on stdin; JSON only on stdout."""
import csv
import ctypes
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import zipfile
import xml.etree.ElementTree as ET

VERSION = 'extract-1'
MAX_INPUT = 100 * 1024 * 1024
MAX_XML = 32 * 1024 * 1024
MAX_TEXT = 1_500_000
MAX_PAGES = 200
NS = {'w': 'http://schemas.openxmlformats.org/wordprocessingml/2006/main',
      's': 'http://schemas.openxmlformats.org/spreadsheetml/2006/main',
      'a': 'http://schemas.openxmlformats.org/drawingml/2006/main'}

class ExtractionError(Exception):
    pass

def lower_priority():
    if os.name == 'nt':
        kernel = ctypes.WinDLL('kernel32', use_last_error=True)
        kernel.GetCurrentProcess.restype = ctypes.c_void_p
        handle = kernel.GetCurrentProcess()
        kernel.SetPriorityClass.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
        kernel.SetPriorityClass(handle, 0x00100000)  # PROCESS_MODE_BACKGROUND_BEGIN
        kernel.SetPriorityClass(handle, 0x40)  # IDLE_PRIORITY_CLASS
    else:
        try:
            os.nice(19)
            import resource
            resource.setrlimit(resource.RLIMIT_AS, (1536 * 1024**2, 1536 * 1024**2))
        except (OSError, ValueError, ImportError):
            pass
    os.environ['OMP_THREAD_LIMIT'] = '1'


def block(text, location='', source='text', boxes=None):
    if len(text) > MAX_TEXT:
        raise ExtractionError('Превышен лимит извлекаемого текста')
    return dict(text=text, location=location, source=source, boxes=boxes or [])


def xml(archive, name):
    info = archive.getinfo(name)
    if info.file_size > MAX_XML:
        raise ExtractionError('Слишком большой XML внутри документа')
    data = archive.read(name)
    if b'<!DOCTYPE' in data.upper() or b'<!ENTITY' in data.upper():
        raise ExtractionError('XML с DTD не поддерживается')
    return ET.fromstring(data)


def office(path):
    output = []
    with zipfile.ZipFile(path) as z:
        if len(z.infolist()) > 10000 or sum(i.file_size for i in z.infolist()) > 128 * 1024**2:
            raise ExtractionError('Превышен лимит распакованного документа')
        if path.suffix.lower() == '.docx':
            parts = ['word/document.xml'] + sorted(n for n in z.namelist() if n.startswith(('word/header', 'word/footer', 'word/footnotes', 'word/endnotes')) and n.endswith('.xml'))
            for name in parts:
                root = xml(z, name)
                paragraphs = [''.join(t.text or '' for t in p.findall('.//w:t', NS)) for p in root.findall('.//w:p', NS)]
                output.append(block('\n'.join(paragraphs), name.rsplit('/', 1)[-1]))
        elif path.suffix.lower() == '.pptx':
            slides = sorted((n for n in z.namelist() if n.startswith('ppt/slides/slide') and n.endswith('.xml')), key=lambda n: int(Path(n).stem[5:]))
            for index, name in enumerate(slides):
                root = xml(z, name)
                output.append(block('\n'.join(t.text or '' for t in root.findall('.//a:t', NS)), f'Слайд {index+1}'))
        else:
            shared = []
            if 'xl/sharedStrings.xml' in z.namelist():
                shared = [''.join(t.text or '' for t in si.findall('.//s:t', NS)) for si in xml(z, 'xl/sharedStrings.xml').findall('s:si', NS)]
            relationships = {r.attrib['Id']: r.attrib['Target'] for r in xml(z, 'xl/_rels/workbook.xml.rels') if r.attrib.get('TargetMode') != 'External'}
            workbook = xml(z, 'xl/workbook.xml')
            for sheet in workbook.findall('s:sheets/s:sheet', NS):
                rid = sheet.attrib.get('{http://schemas.openxmlformats.org/officeDocument/2006/relationships}id')
                target = relationships.get(rid, '')
                name = target.lstrip('/') if target.startswith('/') else 'xl/' + target
                if name not in z.namelist():
                    raise ExtractionError('Неподдерживаемая ссылка на лист')
                for cell in xml(z, name).findall('.//s:sheetData/s:row/s:c', NS):
                    value = cell.findtext('s:v', '', NS)
                    kind = cell.attrib.get('t')
                    if kind == 's':
                        value = shared[int(value)] if value else ''
                    elif kind == 'inlineStr':
                        value = ''.join(t.text or '' for t in cell.findall('.//s:t', NS))
                    formula = cell.findtext('s:f', None, NS)
                    text = f'Формула: ={formula}\nСохранённый результат: {value or "отсутствует"}' if formula is not None else value
                    if text:
                        output.append(block(text, f'{sheet.attrib.get("name", "Лист")}!{cell.attrib.get("r", "?")}'))
                    if len(output) > 30000:
                        raise ExtractionError('Превышен лимит ячеек: 30000')
    return output


def ocr_image(image, location, config):
    from PIL import Image, ImageOps, ImageFilter
    image = ImageOps.exif_transpose(image).convert('RGB')
    if image.width * image.height > 40_000_000:
        raise ExtractionError('Изображение превышает 40 мегапикселей')
    image.thumbnail((3600, 3600))
    with tempfile.TemporaryDirectory(prefix='localfind-ocr-') as tmp:
        source = Path(tmp) / 'page.png'
        image.save(source)
        exe = config.get('tesseract') or 'tesseract'
        env = os.environ.copy()
        if config.get('tessdata'):
            env['TESSDATA_PREFIX'] = config['tessdata']
        # OSD is best effort; unavailable/low-confidence orientation is left unchanged.
        orientation = subprocess.run([exe, str(source), 'stdout', '--psm', '0'], capture_output=True, timeout=20, env=env)
        rotation = 0
        orientation_confidence = 0.0
        if orientation.returncode == 0:
            for line in orientation.stdout.decode('utf-8', 'replace').splitlines():
                if line.startswith('Rotate:'):
                    rotation = int(line.split(':', 1)[1].strip())
                if line.startswith('Orientation confidence:'):
                    orientation_confidence = float(line.split(':', 1)[1].strip())
        if orientation_confidence < 5.0:
            rotation = 0
        if rotation:
            image = image.rotate(-rotation, expand=True)
        image = ImageOps.autocontrast(ImageOps.grayscale(image)).filter(ImageFilter.SHARPEN)
        image.save(source)
        proc = subprocess.run([exe, str(source), 'stdout', '-l', 'rus+eng', '--psm', '3', '-c', 'tessedit_create_tsv=1'], capture_output=True, timeout=90, env=env)
        if proc.returncode:
            raise ExtractionError('OCR недоступно: проверьте Tesseract и языки rus, eng, osd')
        rows = csv.DictReader(io.StringIO(proc.stdout.decode('utf-8', 'replace')), delimiter='\t')
        boxes, lines = [], {}
        for row in rows:
            text = (row.get('text') or '').strip()
            if not text:
                continue
            key = (row['block_num'], row['par_num'], row['line_num'])
            lines.setdefault(key, []).append(text)
            boxes.append(dict(text=text, x=int(row['left']), y=int(row['top']), width=int(row['width']), height=int(row['height']), confidence=float(row['conf']), image_width=image.width, image_height=image.height, rotation=rotation))
        return block('\n'.join(' '.join(words) for words in lines.values()), location, 'ocr', boxes)


def extract(request):
    path = Path(request['path'])
    if path.stat().st_size > MAX_INPUT:
        raise ExtractionError('Файл превышает лимит 100 МБ')
    ext = path.suffix.lower()
    stage = request.get('stage', 'text')
    output, needs_ocr = [], False
    if ext in {'.txt', '.md', '.markdown', '.csv'}:
        data = path.read_bytes()
        if data.startswith((b'\xff\xfe', b'\xfe\xff')):
            text = data.decode('utf-16')
        else:
            try:
                text = data.decode('utf-8-sig')
            except UnicodeDecodeError:
                text = data.decode('cp1251')
        if len(text) > MAX_TEXT:
            raise ExtractionError('Превышен лимит извлекаемого текста')
        output = [block(text)]
    elif ext in {'.docx', '.xlsx', '.pptx'}:
        output = office(path)
    elif ext == '.pdf':
        import pypdfium2 as pdfium
        with pdfium.PdfDocument(str(path)) as pdf:
            if len(pdf) > MAX_PAGES:
                raise ExtractionError('PDF превышает лимит 200 страниц')
            for i in range(len(pdf)):
                page = pdf[i]
                textpage = page.get_textpage()
                text = textpage.get_text_range()
                textpage.close()
                has_text = sum(c.isalnum() for c in text) >= 20
                if has_text:
                    if stage == 'text':
                        output.append(block(text, f'Страница {i+1}'))
                else:
                    needs_ocr = True
                    if stage == 'ocr':
                        scale = min(2.5, 3600 / max(page.get_width(), page.get_height()))
                        bitmap = page.render(scale=scale)
                        output.append(ocr_image(bitmap.to_pil(), f'Страница {i+1}', request))
                        bitmap.close()
                page.close()
    elif ext in {'.png', '.jpg', '.jpeg', '.tif', '.tiff', '.bmp'}:
        needs_ocr = True
        if stage == 'ocr':
            from PIL import Image, ImageSequence
            Image.MAX_IMAGE_PIXELS = 40_000_000
            with Image.open(path) as image:
                if getattr(image, 'n_frames', 1) > MAX_PAGES:
                    raise ExtractionError('Превышен лимит страниц изображения')
                for i, frame in enumerate(ImageSequence.Iterator(image)):
                    output.append(ocr_image(frame.copy(), f'Страница {i+1}', request))
    else:
        raise ExtractionError('Формат не поддерживается')
    if sum(len(b['text']) for b in output) > MAX_TEXT:
        raise ExtractionError('Превышен лимит извлекаемого текста')
    return dict(blocks=output, needs_ocr=needs_ocr, version=VERSION)


def processor_signature(config):
    import hashlib
    from importlib.metadata import version
    digest = hashlib.sha256((VERSION + ':pdf20:psm3:rus+eng').encode())
    for package in ['pypdfium2', 'Pillow']:
        digest.update((package + version(package)).encode())
    exe = config.get('tesseract') or 'tesseract'
    env = os.environ.copy()
    if config.get('tessdata'):
        env['TESSDATA_PREFIX'] = config['tessdata']
    digest.update(json.dumps(config, sort_keys=True).encode())
    try:
        result = subprocess.run([exe, '--version'], capture_output=True, timeout=3, env=env)
        digest.update(result.stdout)
        result = subprocess.run([exe, '--list-langs'], capture_output=True, timeout=3, env=env)
        line = result.stdout.decode('utf-8', 'replace').splitlines()[0]
        directory = Path(config.get('tessdata') or line.split('"')[1])
        for language in ['rus', 'eng', 'osd']:
            model = directory / (language + '.traineddata')
            if model.is_file():
                with model.open('rb') as stream:
                    for chunk in iter(lambda: stream.read(65536), b''):
                        digest.update(chunk)
            else:
                digest.update((language + ':missing').encode())
    except (OSError, subprocess.TimeoutExpired, IndexError):
        digest.update(b'ocr-unavailable')
    return {'version': digest.hexdigest()}

def main():
    lower_priority()
    try:
        request = json.load(sys.stdin)
        response = processor_signature(request) if request.get('operation') == 'signature' else extract(request)
    except ExtractionError as exc:
        response = {'error': str(exc)}
    except Exception as exc:
        # Never include parser messages, document text or passwords in diagnostics.
        response = {'error': 'Не удалось обработать файл (' + type(exc).__name__ + '): повреждён, защищён или недоступен'}
    print(json.dumps(response, ensure_ascii=False))

if __name__ == '__main__':
    main()
