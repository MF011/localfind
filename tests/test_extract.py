import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import zipfile

spec = importlib.util.spec_from_file_location('extract', Path(__file__).resolve().parents[1] / 'workers/extract.py')
extract = importlib.util.module_from_spec(spec)
spec.loader.exec_module(extract)

class ExtractionTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
    def tearDown(self):
        self.tmp.cleanup()
    def archive(self, name, files):
        path = self.root / name
        with zipfile.ZipFile(path, 'w') as z:
            for n, text in files.items(): z.writestr(n, text)
        return path
    def test_text_encoding(self):
        for encoding in ['utf-8-sig', 'utf-16', 'cp1251']:
            p = self.root / 'Текст.txt'; p.write_bytes('Счёт AB-102, invoice'.encode(encoding))
            self.assertIn('Счёт AB-102', extract.extract({'path':str(p)})['blocks'][0]['text'])
    def test_docx_and_pptx(self):
        p = self.archive('a.docx', {'word/document.xml':'<w:document xmlns:w="'+extract.NS['w']+'"><w:body><w:p><w:r><w:t>Русский договор AB-102</w:t></w:r></w:p></w:body></w:document>'})
        self.assertIn('AB-102', extract.extract({'path':str(p)})['blocks'][0]['text'])
        p = self.archive('a.pptx', {'ppt/slides/slide1.xml':'<root xmlns:a="'+extract.NS['a']+'"><a:p><a:r><a:t>Presentation Презентация</a:t></a:r></a:p></root>'})
        b = extract.extract({'path':str(p)})['blocks'][0]
        self.assertEqual(b['location'], 'Слайд 1'); self.assertIn('Презентация', b['text'])
    def test_spreadsheet_formulas_cached_values(self):
        p = self.archive('a.xlsx', {
            'xl/workbook.xml':'<workbook xmlns="'+extract.NS['s']+'" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Продажи" r:id="rId1"/></sheets></workbook>',
            'xl/_rels/workbook.xml.rels':'<Relationships><Relationship Id="rId1" Target="worksheets/sheet1.xml"/></Relationships>',
            'xl/worksheets/sheet1.xml':'<worksheet xmlns="'+extract.NS['s']+'"><sheetData><row><c r="A1" t="inlineStr"><is><t>Код AB-102</t></is></c><c r="B1"><f>1+2</f><v>3</v></c><c r="C1"><f>SUM(A1:B1)</f></c></row></sheetData></worksheet>'})
        blocks = extract.extract({'path':str(p)})['blocks']
        self.assertEqual(blocks[0]['location'], 'Продажи!A1'); self.assertIn('Сохранённый результат: 3', blocks[1]['text']); self.assertIn('отсутствует', blocks[2]['text'])
    def test_mixed_pdf(self):
        from reportlab.pdfgen import canvas
        from PIL import Image, ImageDraw
        p = self.root / 'mixed.pdf'; img = Image.new('RGB',(800,180),'white'); ImageDraw.Draw(img).text((20,40),'SCANNED PAGE AB-102',fill='black',font_size=42); image = self.root/'page.png'; img.save(image)
        c = canvas.Canvas(str(p)); c.drawString(50,750,'Embedded invoice contract AB-123 and text'); c.showPage(); c.drawImage(str(image),40,500,width=500,height=112); c.save()
        result = extract.extract({'path':str(p)})
        self.assertTrue(result['needs_ocr']);self.assertEqual(len(result['blocks']),1);self.assertEqual(result['blocks'][0]['location'],'Страница 1')
        if os.getenv('LOCALFIND_TEST_TESSDATA'):
            result = extract.extract({'path':str(p),'stage':'ocr','tessdata':os.environ['LOCALFIND_TEST_TESSDATA']})
            self.assertEqual(len(result['blocks']),1);self.assertEqual(result['blocks'][0]['location'],'Страница 2');self.assertIn('AB-102', result['blocks'][0]['text'])
    def test_corrupt_and_password_protected(self):
        from reportlab.pdfgen import canvas
        from reportlab.lib.pdfencrypt import StandardEncryption
        for name in ['broken.pdf', 'locked.pdf']:
            path = self.root/name
            if name.startswith('broken'): path.write_bytes(b'invalid PDF')
            else:
                c = canvas.Canvas(str(path),encrypt=StandardEncryption('private'));c.drawString(20,20,'Private contents');c.save()
            proc=subprocess.run([os.sys.executable,str(Path(extract.__file__)),],input=json.dumps({'path':str(path)}),text=True,capture_output=True)
            response=json.loads(proc.stdout);self.assertIn('error',response);self.assertNotIn('Private contents',response['error'])
    def test_model_change_invalidates_signature(self):
        import shutil
        if not shutil.which('tesseract'): self.skipTest('Tesseract not installed')
        models=self.root/'models';models.mkdir()
        for name in ['rus','eng','osd']:(models/(name+'.traineddata')).write_bytes(b'fixture-model')
        config={'tessdata':str(models)}
        before=extract.processor_signature(config)['version']
        (models/'rus.traineddata').write_bytes(b'updated-fixture-model')
        self.assertNotEqual(before,extract.processor_signature(config)['version'])
    def test_dtd_rejected(self):
        p=self.archive('unsafe.docx',{'word/document.xml':'<!DOCTYPE x [<!ENTITY e "unsafe">]><x>&e;</x>'})
        with self.assertRaises(extract.ExtractionError):extract.extract({'path':str(p)})

if __name__ == '__main__':unittest.main()
