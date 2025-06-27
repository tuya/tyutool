#!/usr/bin/env python
# -*- coding: utf-8 -*-

import os
import sys
import base64

root_re = os.path.join(__file__, "../..")
root = os.path.abspath(root_re)
logo_file_re = "tyutool/gui/ui_logo.py"
logo_file = os.path.join(root, logo_file_re)

logo_context = '''
#!/usr/bin/env python
# -*- coding: utf-8 -*-

'''

logo_ico = os.path.join(root, "resource", "logo.ico")
logo_png = os.path.join(root, "resource", "logo.png")

with open(logo_ico, 'rb') as f:
    icon_str = base64.b64encode(f.read())
    logo_context += 'LOGO_ICON_BYTES = ' + str(icon_str)
    logo_context += "\n\n"

with open(logo_png, 'rb') as f:
    png_str = base64.b64encode(f.read())
    logo_context += 'LOGO_PNG_BYTES = ' + str(png_str)
    logo_context += "\n\n"

with open(logo_file, 'w', encoding='utf-8') as f:
    f.write(logo_context)
