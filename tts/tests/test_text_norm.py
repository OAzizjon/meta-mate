"""Text preparation stays testable without installing the speech models."""

import importlib.util
from pathlib import Path
import sys
from types import ModuleType, SimpleNamespace
import unittest
from unittest.mock import MagicMock, Mock, patch

from text_norm import normalize


class NormalizeTests(unittest.TestCase):
    def test_russian_genitive_after_the_preposition(self):
        cases = {
            'Шаг 2 из 3': 'Шаг два из трёх',
            'из 5': 'из пяти', 'из 21': 'из двадцати одного',
            'ИЗ 4': 'ИЗ четырёх', 'из\n12': 'из двенадцати',
            'из 0': 'из нуля', 'из 40': 'из сорока',
            'из 100': 'из ста', 'из 125': 'из ста двадцати пяти',
            'из 1000': 'из одной тысячи', 'из 2000': 'из двух тысяч',
            'из 21000': 'из двадцати одной тысячи',
            'из 0003': 'из трёх',
            '3 из 5, 100': 'три из пяти, сто',
            'эскиз 3': 'эскиз три', 'из, 3': 'из, три',
            'из 3.50': 'из три запятая пять ноль',
            'из 3%': 'из три процента',
        }
        for text, expected in cases.items():
            with self.subTest(text=text):
                self.assertEqual(normalize('ru', text), expected)

    def test_abbreviations(self):
        cases = [
            ('ru', 'т.е.', 'то есть'), ('ru', 'т. е.', 'то есть'),
            ('ru', 'т.к.', 'так как'), ('ru', 'т. к.', 'так как'),
            ('ru', 'и т.д.', 'и так далее'), ('ru', 'и т. д.', 'и так далее'),
            ('ru', 'и т.п.', 'и тому подобное'),
            ('ru', 'и т. п.', 'и тому подобное'), ('ru', 'см.', 'смотрите'),
            ('ru', 'Т. Е. из 3', 'то есть из трёх'),
            ('ru', 'см. Settings: 100', 'смотрите Сеттингс, сто'),
            ('ru', 'рит.е.', 'рит.е.'), ('ru', 'т.е.слово', 'т.е.слово'),
            ('en', 'e.g.', 'for example'), ('en', 'e. g.', 'for example'),
            ('en', 'i.e.', 'that is'), ('en', 'i. e.', 'that is'),
            ('en', 'etc.', 'et cetera'), ('en', 'vs.', 'versus'),
            ('en', 'E.G. 100 vs. 125, etc.',
             'for example one hundred versus one hundred twenty-five, et cetera'),
            ('en', '(i.e., 2)', 'that is, two'),
            ('en', 'some.g.', 'some.g.'), ('en', 'e.g.example', 'e.g.example'),
        ]
        for lang, text, expected in cases:
            with self.subTest(lang=lang, text=text):
                self.assertEqual(normalize(lang, text), expected)

    def test_integers(self):
        cases = [
            (0, 'zero', 'ноль'), (7, 'seven', 'семь'), (11, 'eleven', 'одиннадцать'),
            (21, 'twenty-one', 'двадцать один'), (100, 'one hundred', 'сто'),
            (125, 'one hundred twenty-five', 'сто двадцать пять'),
            (1000, 'one thousand', 'одна тысяча'),
            (2026, 'two thousand twenty-six', 'две тысячи двадцать шесть'),
            (999999, 'nine hundred ninety-nine thousand nine hundred ninety-nine',
             'девятьсот девяносто девять тысяч девятьсот девяносто девять'),
        ]
        for number, en, ru in cases:
            for lang, expected in [('en', en), ('ru', ru)]:
                with self.subTest(number=number, lang=lang):
                    self.assertEqual(normalize(lang, str(number)), expected)

    def test_percent(self):
        cases = [
            ('1%', 'one percent', 'один процент'),
            ('2%', 'two percent', 'два процента'),
            ('5%', 'five percent', 'пять процентов'),
            ('21%', 'twenty-one percent', 'двадцать один процент'),
            ('125%', 'one hundred twenty-five percent', 'сто двадцать пять процентов'),
            ('11%', 'eleven percent', 'одиннадцать процентов'),
            ('12%', 'twelve percent', 'двенадцать процентов'),
            ('14%', 'fourteen percent', 'четырнадцать процентов'),
            ('22 %', 'twenty-two percent', 'двадцать два процента'),
            ('111%', 'one hundred eleven percent', 'сто одиннадцать процентов'),
        ]
        for text, en, ru in cases:
            for lang, expected in [('en', en), ('ru', ru)]:
                with self.subTest(text=text, lang=lang):
                    self.assertEqual(normalize(lang, text), expected)

    def test_decimals(self):
        for separator in ('.', ','):
            self.assertEqual(normalize('en', f'12{separator}05'), 'twelve point zero five')
            self.assertEqual(normalize('ru', f'12{separator}05'), 'двенадцать запятая ноль пять')
        self.assertEqual(normalize('en', '0.50%'), 'zero point five zero percent')
        self.assertEqual(normalize('ru', '0.50%'), 'ноль запятая пять ноль процентов')

    def test_exceptions(self):
        cases = {
            'Settings': 'Сеттингс', 'Windows': 'Виндоус', 'Chrome': 'Хром',
            'Google': 'Гугл', 'Telegram': 'Телеграм', 'Word': 'Ворд',
            'Excel': 'Эксель', 'Edge': 'Эдж', 'OK': 'Окей', 'Save': 'Сейв',
            'File': 'Файл', 'Display': 'Дисплей', 'Accessibility': 'Аксессибилити',
            'Text': 'Текст', 'Size': 'Сайз', 'Start': 'Старт', 'Enter': 'Энтер',
        }
        for word, expected in cases.items():
            with self.subTest(word=word):
                self.assertEqual(normalize('ru', word), expected)
                self.assertEqual(normalize('ru', word.lower()), expected.lower())
                self.assertEqual(normalize('ru', word.upper()), expected)

    def test_phonetics(self):
        cases = {
            'shop': 'шоп', 'check': 'чек', 'think': 'тинк', 'phone': 'фон',
            'book': 'бук', 'see': 'си', 'back': 'бак', 'quick': 'квик',
            'station': 'сташн', 'box': 'бокс', 'window': 'виндов',
            'city': 'сити', 'jam': 'джам', 'yes': 'йес', 'toy': 'той',
            'cake': 'как', 'me': 'ме', 'cycle': 'сикл',
        }
        for word, expected in cases.items():
            with self.subTest(word=word):
                self.assertEqual(normalize('ru', word), expected)

    def test_mixed_sentence(self):
        result = normalize('ru', 'Нажмите Settings, затем Display: 125%.')
        # The colon stays a pause (a comma the voice can make).
        self.assertEqual(result, 'Нажмите Сеттингс, затем Дисплей, сто двадцать пять процентов.')
        self.assertNotRegex(result, '[A-Za-z0-9]')
        self.assertEqual(normalize('en', 'Step 2 of 3'), 'Step two of three')

    def test_shortcuts_and_symbols(self):
        self.assertEqual(normalize('en', 'Ctrl+Plus & Alt/Shift @ Win Esc Tab Enter'),
                         'Control plus Plus and Alt Shift at Win Escape Tab Enter')
        self.assertEqual(normalize('ru', 'Ctrl+Plus & Alt/Shift @ Win Esc Tab Enter'),
                         'Контрол плюс Плюс и Альт Шифт собака Вин Эскейп Таб Энтер')
        self.assertEqual(normalize('en', '+ & @'), 'plus and at')
        self.assertEqual(normalize('ru', '+ & @'), 'плюс и собака')

    def test_filter_and_empty(self):
        self.assertEqual(normalize('en', "Нажмите Save, затем File! Ёж?"), 'Save, File!?')
        self.assertEqual(normalize('en', "It's  easy-to-use. Really?"), "It's easy-to-use. Really?")
        for lang in ('en', 'ru'):
            for text in ('', ' \n\t ', '...!? -', '/ : # $ % = 😀', '你好'):
                with self.subTest(lang=lang, text=text):
                    self.assertEqual(normalize(lang, text), '')
        self.assertEqual(normalize('en', 'Только кириллица'), '')
        self.assertEqual(normalize('ru', '  ёж\t идёт\n'), 'ёж идёт')

    def test_large_and_zero_padded_numbers(self):
        self.assertEqual(normalize('en', '0007'), 'seven')
        self.assertEqual(normalize('ru', '1000000'), 'один ноль ноль ноль ноль ноль ноль')
        self.assertEqual(normalize('en', '9' * 5000), ' '.join(['nine'] * 5000))

    def test_unsupported_language(self):
        with self.assertRaises(ValueError):
            normalize('fr', 'bonjour')


class SynthesizeTests(unittest.TestCase):
    def test_normalizes_before_loading_and_tokenizing(self):
        # Import the real wrapper with model libraries stubbed: no weights or dependencies.
        numpy = ModuleType('numpy')
        numpy.ndarray, numpy.float32 = object, 'float32'
        torch = ModuleType('torch')
        torch.no_grad = MagicMock()
        transformers = ModuleType('transformers')
        transformers.AutoTokenizer = transformers.VitsModel = object
        spec = importlib.util.spec_from_file_location('mms_under_test', Path(__file__).parents[1] / 'mms.py')
        mms = importlib.util.module_from_spec(spec)
        with patch.dict(sys.modules, {'numpy': numpy, 'torch': torch, 'transformers': transformers}), \
                patch.dict('os.environ'):
            spec.loader.exec_module(mms)
        samples = Mock()
        model = Mock(config=SimpleNamespace(sampling_rate=16000))
        model.return_value.waveform = [Mock(numpy=Mock(return_value=samples))]
        tokenizer = Mock(return_value={})
        with patch.object(mms, 'load', return_value=(model, tokenizer)) as load:
            result, rate = mms.synthesize('en', '125%')
            tokenizer.assert_called_once_with('one hundred twenty-five percent', return_tensors='pt')
            self.assertIs(result, samples.astype.return_value)
            self.assertEqual(rate, 16000)
            load.reset_mock()
            with self.assertRaisesRegex(ValueError, '^nothing to say$'):
                mms.synthesize('en', '...')
            load.assert_not_called()


if __name__ == '__main__':
    unittest.main()
