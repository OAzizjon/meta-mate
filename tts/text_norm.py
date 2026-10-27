"""Speakable text for the English and Russian MMS alphabets (stdlib only)."""

import re


_EN_SMALL = ('zero one two three four five six seven eight nine ten eleven twelve '
             'thirteen fourteen fifteen sixteen seventeen eighteen nineteen').split()
_EN_TENS = 'zero ten twenty thirty forty fifty sixty seventy eighty ninety'.split()
_RU_SMALL = ('ноль один два три четыре пять шесть семь восемь девять десять одиннадцать '
             'двенадцать тринадцать четырнадцать пятнадцать шестнадцать семнадцать '
             'восемнадцать девятнадцать').split()
_RU_TENS = 'ноль десять двадцать тридцать сорок пятьдесят шестьдесят семьдесят восемьдесят девяносто'.split()
_RU_HUNDREDS = 'ноль сто двести триста четыреста пятьсот шестьсот семьсот восемьсот девятьсот'.split()
_KEYS = {
    'ctrl': ('control', 'контрол'), 'alt': ('alt', 'альт'),
    'shift': ('shift', 'шифт'), 'win': ('win', 'вин'),
    'esc': ('escape', 'эскейп'), 'tab': ('tab', 'таб'), 'enter': ('enter', 'энтер'),
}
_EXCEPTIONS = dict(zip(
    'settings windows chrome google telegram word excel edge ok save file display accessibility text size start enter plus minus'.split(),
    'сеттингс виндоус хром гугл телеграм ворд эксель эдж окей сейв файл дисплей аксессибилити текст сайз старт энтер плюс минус'.split(),
))
_DIGRAPHS = {'tion': 'шн', 'sh': 'ш', 'ch': 'ч', 'th': 'т', 'ph': 'ф',
             'oo': 'у', 'ee': 'и', 'ck': 'к', 'qu': 'кв'}
_LETTERS = dict(zip('abdefghijklmnopqr stuvwxz'.replace(' ', ''),
                    ('а', 'б', 'д', 'е', 'ф', 'г', 'х', 'и', 'дж', 'к', 'л', 'м',
                     'н', 'о', 'п', 'к', 'р', 'с', 'т', 'у', 'в', 'в', 'кс', 'з')))
_NUMBER = re.compile(r'[0-9]+(?:[.,][0-9]+)?(?:\s*%)?')
_TIME = re.compile(r'(?<![\w:])([01]?[0-9]|2[0-3]):([0-5][0-9])(?![\w:])')
_ABBREVIATIONS = {
    'en': {r'e\.\s*g\.': 'for example', r'i\.\s*e\.': 'that is',
           r'etc\.': 'et cetera', r'vs\.': 'versus'},
    'ru': {r'т\.\s*е\.': 'то есть', r'т\.\s*к\.': 'так как',
           r'и\s+т\.\s*д\.': 'и так далее',
           r'и\s+т\.\s*п\.': 'и тому подобное', r'см\.': 'смотрите'},
}
_RU_GENITIVE = dict(zip(
    _RU_SMALL + _RU_TENS[2:] + _RU_HUNDREDS[1:] + ['одна', 'две', 'тысяча', 'тысячи', 'тысяч'],
    ('нуля одного двух трёх четырёх пяти шести семи восьми девяти десяти одиннадцати '
     'двенадцати тринадцати четырнадцати пятнадцати шестнадцати семнадцати '
     'восемнадцати девятнадцати двадцати тридцати сорока пятидесяти шестидесяти '
     'семидесяти восьмидесяти девяноста ста двухсот трёхсот четырёхсот пятисот '
     'шестисот семисот восьмисот девятисот одной двух тысячи тысяч тысяч').split(),
))


def _form(n: int, one: str, few: str, many: str) -> str:
    if 11 <= n % 100 <= 14:
        return many
    return one if n % 10 == 1 else few if 2 <= n % 10 <= 4 else many


def _integer(lang: str, n: int, feminine: bool = False) -> str:
    small = _EN_SMALL if lang == 'en' else _RU_SMALL
    if n > 999999:
        return ' '.join(small[int(digit)] for digit in str(n))
    if n < 20:
        if lang == 'ru' and feminine and n in (1, 2):
            return 'одна' if n == 1 else 'две'
        return small[n]
    if n < 100:
        tens = (_EN_TENS if lang == 'en' else _RU_TENS)[n // 10]
        return tens + (('-' if lang == 'en' else ' ') + _integer(lang, n % 10, feminine)
                       if n % 10 else '')
    if n < 1000:
        hundreds = small[n // 100] + ' hundred' if lang == 'en' else _RU_HUNDREDS[n // 100]
        return hundreds + (' ' + _integer(lang, n % 100, feminine) if n % 100 else '')
    thousands, rest = divmod(n, 1000)
    head = _integer(lang, thousands, feminine=lang == 'ru')
    head += ' thousand' if lang == 'en' else ' ' + _form(thousands, 'тысяча', 'тысячи', 'тысяч')
    return head + (' ' + _integer(lang, rest) if rest else '')


def _number(lang: str, match: re.Match) -> str:
    raw = match.group()
    percent = raw.endswith('%')
    parts = re.split(r'[.,]', raw.removesuffix('%').strip())
    # Avoid huge int conversions on arbitrary input; the supported range is six digits.
    whole = parts[0].lstrip('0') or '0'
    small = _EN_SMALL if lang == 'en' else _RU_SMALL
    result = (_integer(lang, int(whole)) if len(whole) <= 6
              else ' '.join(small[int(digit)] for digit in whole))
    if (lang == 'ru' and len(parts) == 1 and not percent
            and re.search(r'\bиз\s+$', match.string[:match.start()], re.IGNORECASE)):
        result = ' '.join(_RU_GENITIVE[word] for word in result.split())
    if len(parts) == 2:
        result += (' point ' if lang == 'en' else ' запятая ')
        result += ' '.join(small[int(digit)] for digit in parts[1])
    if percent:
        suffix = ('percent' if lang == 'en' else 'процентов' if len(parts) == 2
                  else _form(int(whole[-2:]), 'процент', 'процента', 'процентов'))
        result += ' ' + suffix
    return ' ' + result + ' '


def _word(lang: str, word: str) -> str:
    lower = word.lower()
    if lower in _KEYS:
        result = _KEYS[lower][lang == 'ru']
    elif lang == 'en':
        return word
    elif lower in _EXCEPTIONS:
        result = _EXCEPTIONS[lower]
    else:
        if len(lower) > 3 and lower.endswith('e') and lower[-2] not in 'aeiouy':
            lower = lower[:-1]
        pieces = []
        i = 0
        while i < len(lower):
            for source, target in _DIGRAPHS.items():
                if lower.startswith(source, i):
                    pieces.append(target)
                    i += len(source)
                    break
            else:
                char = lower[i]
                if char == 'c':
                    pieces.append('с' if lower[i + 1:i + 2] in ('e', 'i', 'y') else 'к')
                elif char == 'y':
                    pieces.append('й' if i == 0 or lower[i - 1] in 'aeiou' else 'и')
                else:
                    pieces.append(_LETTERS[char])
                i += 1
        result = ''.join(pieces)
    return result[:1].upper() + result[1:] if word[0].isupper() else result


def normalize(lang: str, text: str) -> str:
    """Expand abbreviations/numbers/symbols, then keep the voice's alphabet.

    Decimal fractions are spoken digit by digit, including trailing zeros.
    Clock times use cardinal hours and minutes; years use cardinal integers.
    Integers above 999999 fall back to digit spelling. Unsupported marks become
    spaces so adjacent words do not run together. Punctuation alone is silent.
    """
    if lang not in ('en', 'ru'):
        raise ValueError(f'no MMS-TTS checkpoint for {lang!r}')
    # Match raw dotted forms before number spacing or Latin transliteration.
    for pattern, expansion in _ABBREVIATIONS[lang].items():
        text = re.sub(r'(?<!\w)' + pattern + r'(?!\w)', expansion, text, flags=re.IGNORECASE)
    text = _TIME.sub(lambda match: ' '.join(_integer(lang, int(part))
                                           for part in match.groups()), text)
    text = _NUMBER.sub(lambda match: _number(lang, match), text)
    symbols = {'+': ('plus', 'плюс'), '&': ('and', 'и'), '@': ('at', 'собака')}
    text = re.sub(r'[+&@]', lambda match: ' ' + symbols[match.group()][lang == 'ru'] + ' ', text)
    text = re.sub(r'[A-Za-z]+', lambda match: _word(lang, match.group()), text)
    alphabet = 'A-Za-z' if lang == 'en' else 'А-Яа-яЁё'
    text = re.sub(r'[:;]', ',', text)  # a pause the voice can make
    text = re.sub(r"[^" + alphabet + r"\s.,!?'\-]", ' ', text)
    text = ' '.join(text.split())
    text = re.sub(r'\s+([.,!?])', r'\1', text)
    return text if re.search('[' + alphabet + ']', text) else ''
