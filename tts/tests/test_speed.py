"""Speed handling with stub model libraries: no weights, GPU or network."""

import importlib.util
import io
import json
from pathlib import Path
import sys
import threading
from types import ModuleType, SimpleNamespace
import unittest
from unittest.mock import MagicMock, Mock, patch


def import_file(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).parents[1] / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class SpeedTests(unittest.TestCase):
    def setUp(self):
        numpy = ModuleType('numpy')
        numpy.ndarray, numpy.float32 = object, 'float32'
        torch = ModuleType('torch')
        torch.no_grad = MagicMock()
        transformers = ModuleType('transformers')
        transformers.AutoTokenizer = transformers.VitsModel = object
        with patch.dict(sys.modules, {'numpy': numpy, 'torch': torch, 'transformers': transformers}), \
                patch.dict('os.environ'):
            self.mms = import_file('mms_speed_test', 'mms.py')
        self.model = Mock(speaking_rate=1.0, config=SimpleNamespace(sampling_rate=16000))
        self.model.return_value.waveform = [Mock()]
        self.tokenizer = Mock(return_value={})
        self.mms._loaded['en'] = (self.model, self.tokenizer)

    def test_validation_before_model_loading(self):
        for speed in (True, False, None, '1', [], {}, 0.79, 1.31,
                      float('nan'), float('inf'), -float('inf')):
            with self.subTest(speed=speed), patch.object(self.mms, 'load') as load:
                with self.assertRaises(ValueError):
                    self.mms.synthesize('en', 'Hi', speed)
                load.assert_not_called()

    def test_speed_is_applied_and_default_is_restored(self):
        observed = []
        waveform = self.model.return_value

        def generate(**_inputs):
            observed.append(self.model.speaking_rate)
            return waveform

        self.model.side_effect = generate
        for speed in (0.8, 1, 1.05, 1.3):
            self.mms.synthesize('en', 'Hi', speed)
            self.assertEqual(self.model.speaking_rate, 1.0)
        self.mms.synthesize('en', 'Hi')
        self.assertEqual(observed, [0.8, 1, 1.05, 1.3, 1.0])

    def test_speaking_rate_restored_after_generation_error(self):
        self.model.speaking_rate = 0.95
        self.model.side_effect = RuntimeError('model failed')
        with self.assertRaises(RuntimeError):
            self.mms.synthesize('en', 'Hi', 1.3)
        self.assertEqual(self.model.speaking_rate, 0.95)

    def test_concurrent_requests_cannot_change_an_active_rate(self):
        entered, release, attempted = threading.Event(), threading.Event(), threading.Event()
        observed, errors = [], []
        waveform = self.model.return_value

        def generate(**_inputs):
            observed.append(self.model.speaking_rate)
            if len(observed) == 1:
                entered.set()
                if not release.wait(5):
                    raise RuntimeError('test timed out')
                observed.append(self.model.speaking_rate)
            return waveform

        def speak(speed):
            try:
                self.mms.synthesize('en', 'Hi', speed)
            except Exception as error:
                errors.append(error)

        real_load = self.mms.load

        def load(lang):
            if threading.current_thread().name == 'second':
                attempted.set()
            return real_load(lang)

        self.model.side_effect = generate
        first = threading.Thread(target=speak, args=(0.8,))
        second = threading.Thread(target=speak, args=(1.3,), name='second')
        with patch.object(self.mms, 'load', side_effect=load):
            first.start()
            try:
                self.assertTrue(entered.wait(5))
                second.start()
                self.assertTrue(attempted.wait(5))
                self.assertEqual(self.model.speaking_rate, 0.8)
            finally:
                release.set()
                first.join(5)
                if second.ident is not None:
                    second.join(5)
        self.assertFalse(first.is_alive())
        self.assertFalse(second.is_alive())
        self.assertEqual(errors, [])
        self.assertEqual(observed, [0.8, 0.8, 1.3])
        self.assertEqual(self.model.speaking_rate, 1.0)

    def test_dev_endpoint_validates_and_forwards_speed(self):
        with patch.dict(sys.modules, {'mms': self.mms}):
            service = import_file('serve_dev_speed_test', 'serve_dev.py')

        def request(payload):
            raw = json.dumps(payload).encode()
            handler = service.Handler.__new__(service.Handler)
            handler.path, handler.secret = '/synthesize', 'test-secret'
            handler.headers = {'X-MMS-Secret': 'test-secret', 'Content-Length': str(len(raw))}
            handler.rfile, handler.wfile = io.BytesIO(raw), io.BytesIO()
            handler.send_response, handler.send_header, handler.end_headers = Mock(), Mock(), Mock()
            handler.do_POST()
            return handler.send_response.call_args.args[0]

        with patch.object(self.mms, 'synthesize', return_value=([], 16000)) as synthesize, \
                patch.object(self.mms, 'to_wav', return_value=b'RIFF'):
            for speed in (0.8, 1, 1.3):
                self.assertEqual(request({'lang': 'en', 'text': 'Hi', 'speed': speed}), 200)
                synthesize.assert_called_with('en', 'Hi', speed)
            self.assertEqual(request({'lang': 'en', 'text': 'Hi'}), 200)
            synthesize.assert_called_with('en', 'Hi', 1.0)
            synthesize.reset_mock()
            for speed in (True, None, '1', 0.7, 1.4, float('nan'), float('inf')):
                self.assertEqual(request({'lang': 'en', 'text': 'Hi', 'speed': speed}), 422)
            synthesize.assert_not_called()

    def test_modal_endpoint_validates_and_forwards_speed(self):
        try:
            from fastapi.testclient import TestClient
        except ImportError:
            self.skipTest('FastAPI is not installed in this interpreter')
        # Import the actual endpoint without Modal's image build or deployment.
        def decorator(**_options):
            return lambda target: target

        modal = ModuleType('modal')
        modal.Image = MagicMock()
        modal.Secret = Mock()
        modal.App = Mock(return_value=SimpleNamespace(cls=decorator))
        modal.concurrent = modal.enter = modal.asgi_app = decorator
        with patch.dict(sys.modules, {'modal': modal, 'mms': self.mms}):
            service = import_file('modal_speed_test', 'modal_app.py')
            instance = service.Tts()
            instance.secret = b'test-secret'
            api = instance.web()
        with TestClient(api) as client, \
                patch.object(self.mms, 'synthesize', return_value=([], 16000)) as synthesize, \
                patch.object(self.mms, 'to_wav', return_value=b'RIFF'):
            headers = {'X-MMS-Secret': 'test-secret'}

            def request(payload):
                return client.post('/synthesize', content=json.dumps(payload), headers=headers)

            for speed in (0.8, 1, 1.3):
                self.assertEqual(request({'lang': 'en', 'text': 'Hi', 'speed': speed}).status_code, 200)
                synthesize.assert_called_with('en', 'Hi', speed)
            self.assertEqual(request({'lang': 'en', 'text': 'Hi'}).status_code, 200)
            synthesize.assert_called_with('en', 'Hi', 1.0)
            synthesize.reset_mock()
            for speed in (True, False, None, '1', [], {}, 0.7, 1.4, float('nan'), float('inf')):
                self.assertEqual(request({'lang': 'en', 'text': 'Hi', 'speed': speed}).status_code, 422)
            synthesize.assert_not_called()
