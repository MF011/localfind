"""Reproducible synthetic OCR comparison. All generated data stays in --work."""
import argparse
import difflib
import importlib.util
import json
import os
from pathlib import Path
import time
from PIL import Image, ImageDraw, ImageFont, ImageFilter

parser=argparse.ArgumentParser()
parser.add_argument('--work',required=True)
parser.add_argument('--tessdata',required=True)
parser.add_argument('--engine',choices=['tesseract','paddle'],required=True)
args=parser.parse_args()
work=Path(args.work).resolve();work.mkdir(parents=True,exist_ok=True)
os.environ['PADDLE_OCR_BASE_DIR']=str(work/'paddle-models')
os.environ['PADDLE_EXTENSION_DIR']=str(work/'paddle-extensions')
os.environ['XDG_CACHE_HOME']=str(work/'cache')
os.environ['MPLCONFIGDIR']=str(work/'matplotlib')
os.environ['OMP_NUM_THREADS']='1'
font=ImageFont.truetype('/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf',36)
samples={
 'ru_clean':'Договор поставки AB-102\nПолучатель: отдел закупок\nДата: 14 ноября 2026 года\nКоличество: 125 единиц\nСумма: 42000 рублей',
 'en_clean':'INVOICE AB-102\nCustomer: Procurement department\nDate: 14 November 2026\nQuantity: 125 units\nTotal amount: 42000 rubles',
 'mixed_blur':'Договор AB-102 / Invoice\nАртикул: RU-550-XL\nProduct code: 987654321\nПоставка: 14.11.2026\nReference: TEST-2048',
 'ru_rotated':'Договор поставки AB-102\nПолучатель: отдел закупок\nДата: 14 ноября 2026 года\nКоличество: 125 единиц\nСумма: 42000 рублей',
}
for name,text in samples.items():
 image=Image.new('RGB',(1150,440),'white');ImageDraw.Draw(image).multiline_text((40,40),text,font=font,fill='black',spacing=27)
 if 'blur' in name:image=image.filter(ImageFilter.GaussianBlur(1.1)).rotate(2,expand=True,fillcolor='white')
 if 'rotated' in name:image=image.rotate(90,expand=True,fillcolor='white')
 image.save(work/f'{name}.png')

def distance(a,b):
 row=list(range(len(b)+1))
 for i,x in enumerate(a,1):
  nextrow=[i]
  for j,y in enumerate(b,1):nextrow.append(min(nextrow[-1]+1,row[j]+1,row[j-1]+(x!=y)))
  row=nextrow
 return row[-1]

def norm(s):return ' '.join(s.casefold().split())
if args.engine=='tesseract':
 spec=importlib.util.spec_from_file_location('extract',Path(__file__).resolve().parents[1]/'workers/extract.py');module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
 def recognize(p):
  with Image.open(p) as image:return module.ocr_image(image,'',{'tessdata':args.tessdata})['text']
else:
 from paddleocr import PaddleOCR
 engine=PaddleOCR(lang='ru',use_gpu=False,ocr_version='PP-OCRv3',use_angle_cls=True,show_log=False,enable_mkldnn=False,cpu_threads=1,det_limit_side_len=1600)
 def recognize(p):
  result=engine.ocr(str(p),cls=True)
  return '\n'.join(line[1][0] for page in (result or []) for line in (page or []))
results=[]
for name,expected in samples.items():
 start=time.perf_counter();actual=recognize(work/f'{name}.png');elapsed=time.perf_counter()-start
 results.append(dict(sample=name,seconds=round(elapsed,3),cer=round(distance(norm(expected),norm(actual))/len(norm(expected)),4),expected=expected,actual=actual))
(work/f'{args.engine}.json').write_text(json.dumps(results,ensure_ascii=False,indent=2))
print(json.dumps(results,ensure_ascii=False,indent=2))
