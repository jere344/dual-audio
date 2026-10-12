"""Dual Audio - share system audio to several output devices at once (Windows)."""

from __future__ import annotations

import json
import math
import os
import queue
import sys
import threading
import time
import tkinter as tk
import warnings

import numpy as np
import customtkinter as ctk
import pystray
import soundcard as sc
import sounddevice as sd
from PIL import Image, ImageDraw

warnings.filterwarnings("ignore", message="data discontinuity")

APP_NAME = "Dual Audio"
APP_ID = "DualAudio"
__version__ = "1.0.0"
SAMPLE_RATE = 48000
BLOCK = 1024
CHANNELS = 2
DTYPE = "float32"
QUEUE_BLOCKS = 64
MAX_DELAY_MS = 2000

CONFIG_DIR = os.path.join(os.environ.get("APPDATA", os.path.expanduser("~")), APP_ID)
CONFIG_PATH = os.path.join(CONFIG_DIR, "settings.json")

BG = "#0f1117"
CARD = "#181b24"
CARD_ALT = "#1f2330"
ACCENT = "#4f46e5"
ACCENT_HOVER = "#6366f1"
TEXT = "#e7e9ee"
MUTED = "#8b93a7"
GOOD = "#22c55e"
BAD = "#ef4444"


def init_com() -> None:
    if sys.platform != "win32":
        return
    try:
        import ctypes

        ctypes.windll.ole32.CoInitializeEx(None, 0)
    except Exception:
        pass


def enable_dpi_awareness() -> None:
    if sys.platform != "win32":
        return
    try:
        from ctypes import windll

        windll.shcore.SetProcessDpiAwareness(1)
    except Exception:
        pass


def load_settings() -> dict:
    try:
        with open(CONFIG_PATH, "r", encoding="utf-8") as handle:
            data = json.load(handle)
            return data if isinstance(data, dict) else {}
    except Exception:
        return {}


def save_settings(data: dict) -> None:
    try:
        os.makedirs(CONFIG_DIR, exist_ok=True)
        with open(CONFIG_PATH, "w", encoding="utf-8") as handle:
            json.dump(data, handle, indent=2)
    except Exception:
        pass


def wasapi_hostapi() -> int | None:
    try:
        for i, api in enumerate(sd.query_hostapis()):
            if "WASAPI" in api["name"]:
                return i
    except Exception:
        pass
    return None


def list_output_devices() -> tuple[list[dict], int | None]:
    idx = wasapi_hostapi()
    devices = []
    for dev in sd.query_devices():
        if dev["max_output_channels"] < CHANNELS:
            continue
        if idx is not None and dev["hostapi"] != idx:
            continue
        devices.append(dev)
    return devices, idx


def default_output_index(idx: int | None) -> int | None:
    try:
        if idx is not None:
            return sd.query_hostapis(idx)["default_output_device"]
        return sd.default.device[1]
    except Exception:
        return None


def safe_name(device, fallback: str = "Default output device") -> str:
    try:
        name = device.name
        if name and not name.strip().startswith("{"):
            return name
    except Exception:
        pass
    return fallback


def pretty_device_name(name: str, index: int) -> str:
    name = (name or "").strip()
    if not name or name.startswith("{"):
        return f"Audio output #{index}"
    return name


def all_loopback_sources() -> list:
    return [
        m for m in sc.all_microphones(include_loopback=True) if getattr(m, "isloopback", False)
    ]


def default_loopback():
    try:
        target_id = sc.default_speaker().id
    except Exception:
        target_id = None
    loopbacks = all_loopback_sources()
    if target_id is not None:
        for mic in loopbacks:
            try:
                if mic.id == target_id:
                    return mic
            except Exception:
                continue
    if not loopbacks:
        raise RuntimeError("no loopback capture device is available")
    return loopbacks[0]


def tone_block(seconds: float, freq: float, volume: float):
    count = int(SAMPLE_RATE * seconds)
    time_axis = np.arange(count) / SAMPLE_RATE
    wave = (volume * np.sin(2 * np.pi * freq * time_axis)).astype(DTYPE)
    return np.ascontiguousarray(np.stack([wave, wave], axis=1))


class Device:
    __slots__ = ("index", "name", "enabled", "volume", "delay_ms", "q", "thread", "error")

    def __init__(self, index: int, name: str) -> None:
        self.index = index
        self.name = name
        self.enabled = False
        self.volume = 1.0
        self.delay_ms = 0
        self.q: queue.Queue | None = None
        self.thread: threading.Thread | None = None
        self.error: str | None = None


class DualAudioEngine:
    def __init__(self, log=None) -> None:
        self._log = log or (lambda _msg: None)
        self.devices: dict[str, Device] = {}
        self._active: list[Device] = []
        self._running = threading.Event()
        self._capture_thread: threading.Thread | None = None
        self.rate = SAMPLE_RATE
        self.source_mic = None
        self.last_peak = 0.0
        self.capture_error: str | None = None

    @property
    def running(self) -> bool:
        return self._running.is_set()

    @property
    def active(self) -> list[Device]:
        return list(self._active)

    def start(self) -> None:
        if self._running.is_set():
            return
        active = [d for d in self.devices.values() if d.enabled]
        if not active:
            raise RuntimeError("Select at least one output device.")
        mic = self.source_mic if self.source_mic is not None else default_loopback()
        self.capture_error = None
        self.last_peak = 0.0
        for dev in active:
            dev.q = queue.Queue(maxsize=QUEUE_BLOCKS)
            dev.error = None
            dev.thread = None
        self._active = active
        self._running.set()
        self._capture_thread = threading.Thread(
            target=self._capture_loop, args=(mic,), name="capture", daemon=True
        )
        self._capture_thread.start()
        for dev in active:
            self._start_device(dev)

    def stop(self) -> None:
        self._running.clear()
        for dev in list(self.devices.values()):
            if dev.q is not None:
                try:
                    dev.q.put_nowait(None)
                except Exception:
                    pass
        if self._capture_thread and self._capture_thread.is_alive():
            self._capture_thread.join(timeout=2)
        for dev in list(self.devices.values()):
            if dev.thread and dev.thread.is_alive():
                dev.thread.join(timeout=2)
            dev.thread = None
        self._active = []
        self._capture_thread = None

    def set_delay(self, dev: Device, delay_ms: int) -> None:
        dev.delay_ms = delay_ms
        if self._running.is_set() and dev in self._active and dev.thread and dev.thread.is_alive():
            self._restart_device(dev)

    def play_test_tone(self, index: int, seconds: float = 0.9, freq: float = 440.0,
                       volume: float = 0.25) -> None:
        def run() -> None:
            init_com()
            try:
                block = tone_block(seconds, freq, volume)
                with sd.OutputStream(
                    device=index, samplerate=SAMPLE_RATE, channels=CHANNELS, dtype=DTYPE
                ) as stream:
                    stream.write(block)
            except Exception as exc:
                self._log(f"Test tone failed on device {index}: {exc}")

        threading.Thread(target=run, name="test-tone", daemon=True).start()

    def enable_device(self, dev: Device) -> None:
        if not self._running.is_set():
            return
        dev.q = queue.Queue(maxsize=QUEUE_BLOCKS)
        dev.error = None
        if dev not in self._active:
            self._active.append(dev)
        self._start_device(dev)

    def disable_device(self, dev: Device) -> None:
        if dev in self._active:
            self._active.remove(dev)
        if dev.q is not None:
            try:
                dev.q.put_nowait(None)
            except Exception:
                pass

    def _restart_device(self, dev: Device) -> None:
        if dev.q is not None:
            try:
                dev.q.put_nowait(None)
            except Exception:
                pass
        if dev.thread:
            dev.thread.join(timeout=1.5)
        dev.q = queue.Queue(maxsize=QUEUE_BLOCKS)
        self._start_device(dev)

    def _start_device(self, dev: Device) -> None:
        dev.thread = threading.Thread(
            target=self._player_loop, args=(dev,), name=f"out:{dev.name}", daemon=True
        )
        dev.thread.start()

    @staticmethod
    def _enqueue(q: queue.Queue, block) -> None:
        try:
            q.put_nowait(block)
        except queue.Full:
            try:
                q.get_nowait()
            except queue.Empty:
                pass
            try:
                q.put_nowait(block)
            except queue.Full:
                pass

    def _capture_loop(self, mic) -> None:
        init_com()
        try:
            with mic.recorder(samplerate=self.rate, channels=CHANNELS, blocksize=BLOCK) as rec:
                while self._running.is_set():
                    data = rec.record(numframes=BLOCK)
                    if data.shape[1] != CHANNELS:
                        data = np.tile(data[:, :1], (1, CHANNELS))
                    base = np.ascontiguousarray(data, dtype=DTYPE)
                    self.last_peak = float(np.abs(base).max())
                    for dev in self._active:
                        vol = dev.volume
                        if vol == 1.0:
                            block = base
                        else:
                            block = base * vol
                            if vol > 1.0:
                                np.clip(block, -1.0, 1.0, out=block)
                        self._enqueue(dev.q, block)
        except Exception as exc:
            self.capture_error = str(exc)
            self._log(f"Capture error: {exc}")
        finally:
            self._running.clear()
            for dev in self._active:
                if dev.q is not None:
                    try:
                        dev.q.put_nowait(None)
                    except Exception:
                        pass

    def _player_loop(self, dev: Device) -> None:
        init_com()
        silence = np.zeros((BLOCK, CHANNELS), dtype=DTYPE)
        try:
            with sd.OutputStream(
                device=dev.index,
                samplerate=self.rate,
                channels=CHANNELS,
                dtype=DTYPE,
                blocksize=BLOCK,
                latency="high",
            ) as stream:
                remaining = int(self.rate * (dev.delay_ms or 0) / 1000.0)
                while remaining > 0 and self._running.is_set():
                    n = min(BLOCK, remaining)
                    stream.write(silence[:n])
                    remaining -= n
                while self._running.is_set():
                    try:
                        block = dev.q.get(timeout=0.4)
                    except queue.Empty:
                        stream.write(silence)
                        continue
                    if block is None:
                        break
                    stream.write(block)
        except Exception as exc:
            dev.error = str(exc)
            self._log(f"Output '{dev.name}' error: {exc}")


def make_icon_image(size: int = 256) -> Image.Image:
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    pad = size * 0.06
    draw.ellipse((pad, pad, size - pad, size - pad), fill=ACCENT)
    band_w = int(size * 0.10)
    draw.arc(
        (size * 0.24, size * 0.20, size * 0.76, size * 0.74),
        start=180,
        end=360,
        fill="white",
        width=band_w,
    )
    cup_w = size * 0.26
    cup_h = size * 0.34
    y0 = size * 0.46
    draw.rounded_rectangle(
        (size * 0.15, y0, size * 0.15 + cup_w, y0 + cup_h), radius=int(cup_w * 0.45), fill="white"
    )
    draw.rounded_rectangle(
        (size * 0.85 - cup_w, y0, size * 0.85, y0 + cup_h), radius=int(cup_w * 0.45), fill="white"
    )
    return img


class DeviceRow:
    def __init__(self, app, parent, dev: Device) -> None:
        self.app = app
        self.dev = dev
        self._state_key = None
        frame = ctk.CTkFrame(parent, fg_color=CARD_ALT, corner_radius=12)
        frame.pack(fill="x", pady=6, padx=4)
        self.frame = frame
        frame.grid_columnconfigure(1, weight=1)
        frame.grid_columnconfigure(3, weight=1)

        self.var = ctk.BooleanVar(value=dev.enabled)
        ctk.CTkCheckBox(
            frame,
            text=dev.name,
            variable=self.var,
            font=ctk.CTkFont(size=14, weight="bold"),
            text_color=TEXT,
            fg_color=ACCENT,
            hover_color=ACCENT_HOVER,
            command=self._on_enable,
        ).grid(row=0, column=0, columnspan=4, sticky="w", padx=14, pady=(12, 2))

        self.state = ctk.CTkLabel(frame, text="", font=ctk.CTkFont(size=12), text_color=MUTED)
        self.state.grid(row=0, column=4, sticky="e", padx=(0, 8))

        self.test_btn = ctk.CTkButton(
            frame, text="Test", width=56, height=24, corner_radius=6,
            fg_color=CARD, hover_color=ACCENT, text_color=TEXT, font=ctk.CTkFont(size=11),
            command=self._on_test,
        )
        self.test_btn.grid(row=0, column=5, sticky="e", padx=(0, 14))

        ctk.CTkLabel(frame, text="Volume", text_color=MUTED, font=ctk.CTkFont(size=12)).grid(
            row=1, column=0, columnspan=2, sticky="w", padx=(14, 0)
        )
        ctk.CTkLabel(frame, text="Sync delay", text_color=MUTED, font=ctk.CTkFont(size=12)).grid(
            row=1, column=2, columnspan=2, sticky="w", padx=(8, 0)
        )

        self.vol_label = ctk.CTkLabel(
            frame, text=f"{int(dev.volume * 100)}%", width=46, text_color=TEXT, anchor="w"
        )
        self.vol_label.grid(row=2, column=0, sticky="w", padx=(14, 0))
        self.vol_slider = ctk.CTkSlider(
            frame, from_=0, to=150, number_of_steps=150, command=self._on_volume,
            progress_color=ACCENT, button_color=ACCENT, button_hover_color=ACCENT_HOVER,
        )
        self.vol_slider.set(dev.volume * 100)
        self.vol_slider.grid(row=2, column=1, sticky="ew", padx=(6, 12), pady=(0, 12))

        self.delay_label = ctk.CTkLabel(
            frame, text=f"{int(dev.delay_ms)} ms", width=64, text_color=TEXT, anchor="w"
        )
        self.delay_label.grid(row=2, column=2, sticky="w", padx=(8, 0))
        self.delay_slider = ctk.CTkSlider(
            frame, from_=0, to=MAX_DELAY_MS, number_of_steps=200, command=self._on_delay,
            progress_color=ACCENT, button_color=ACCENT, button_hover_color=ACCENT_HOVER,
        )
        self.delay_slider.set(dev.delay_ms)
        self.delay_slider.grid(row=2, column=3, sticky="ew", padx=(6, 14), pady=(0, 12))

    def _on_test(self) -> None:
        self.app.engine.play_test_tone(self.dev.index)
        self.set_state("testing...", MUTED)

    def _on_enable(self) -> None:
        self.dev.enabled = self.var.get()
        if self.dev.enabled:
            self.app.engine.enable_device(self.dev)
        else:
            self.app.engine.disable_device(self.dev)
        self.app.persist_settings()

    def _on_volume(self, value: float) -> None:
        self.dev.volume = value / 100.0
        self.vol_label.configure(text=f"{int(round(value))}%")
        self.app.persist_settings()

    def _on_delay(self, value: float) -> None:
        self.dev.delay_ms = int(round(value))
        self.delay_label.configure(text=f"{self.dev.delay_ms} ms")
        self.app.schedule_delay(self.dev)
        self.app.persist_settings()

    def set_state(self, text: str, color: str) -> None:
        if self._state_key == (text, color):
            return
        self._state_key = (text, color)
        self.state.configure(text=text, text_color=color)

    def destroy(self) -> None:
        try:
            self.frame.destroy()
        except Exception:
            pass


class App(ctk.CTk):
    def __init__(self) -> None:
        super().__init__()
        ctk.set_appearance_mode("dark")
        self.configure(fg_color=BG)
        self.title(APP_NAME)
        self.geometry("820x720")
        self.minsize(760, 600)

        self.engine = DualAudioEngine(log=self._log)
        self.settings = load_settings()
        self.rows: list[DeviceRow] = []
        self.icon: pystray.Icon | None = None
        self._quitting = False
        self.source_mics: dict = {}
        self._sharing = False
        self._silent_since = None
        self._save_job = None
        self._delay_jobs: dict = {}
        self._last_level = 0.0
        self._resizing = False
        self._tick_job = None
        self._body_job = None

        self.body = tk.Frame(self, bg=BG)
        self.body.place(x=0, y=0, width=820, height=720)
        self._build_header()
        self._build_source_card()
        self._build_devices_card()
        self._build_footer()

        self.refresh_devices(first=True)
        self.protocol("WM_DELETE_WINDOW", self.hide_to_tray)
        self.bind("<Configure>", self._on_configure)
        self._setup_tray()
        self._tick_job = self.after(400, self._tick)

    def _build_header(self) -> None:
        header = ctk.CTkFrame(self.body, fg_color="transparent")
        header.pack(fill="x", padx=22, pady=(20, 6))
        ctk.CTkLabel(
            header, text=APP_NAME, font=ctk.CTkFont(size=28, weight="bold"), text_color=TEXT
        ).pack(anchor="w")
        ctk.CTkLabel(
            header,
            text="Play your PC's audio through several headsets or speakers at the same time.",
            font=ctk.CTkFont(size=13),
            text_color=MUTED,
        ).pack(anchor="w", pady=(2, 0))

    def _build_source_card(self) -> None:
        card = ctk.CTkFrame(self.body, fg_color=CARD, corner_radius=14)
        card.pack(fill="x", padx=22, pady=(10, 6))
        inner = ctk.CTkFrame(card, fg_color="transparent")
        inner.pack(fill="x", padx=16, pady=14)
        top = ctk.CTkFrame(inner, fg_color="transparent")
        top.pack(fill="x")
        ctk.CTkLabel(
            top, text="SYSTEM AUDIO SOURCE", font=ctk.CTkFont(size=11, weight="bold"), text_color=MUTED
        ).pack(side="left")
        self.level_label = ctk.CTkLabel(top, text="input: -inf dB", text_color=MUTED, font=ctk.CTkFont(size=11))
        self.level_label.pack(side="right")

        row = ctk.CTkFrame(inner, fg_color="transparent")
        row.pack(fill="x", pady=(6, 0))
        self.source_menu = ctk.CTkOptionMenu(
            row,
            values=["System default"],
            command=self._on_source_change,
            fg_color=CARD_ALT,
            button_color=ACCENT,
            button_hover_color=ACCENT_HOVER,
            text_color=TEXT,
            font=ctk.CTkFont(size=13),
            dynamic_resizing=False,
            width=380,
        )
        self.source_menu.pack(side="left")
        self.level_bar = ctk.CTkProgressBar(row, width=150, progress_color=GOOD, fg_color=CARD_ALT)
        self.level_bar.set(0)
        self.level_bar.pack(side="right")

        self.source_label = ctk.CTkLabel(
            inner,
            text="Capturing...",
            font=ctk.CTkFont(size=12),
            text_color=MUTED,
        )
        self.source_label.pack(anchor="w", pady=(6, 0))
        ctk.CTkLabel(
            inner,
            text="Everything playing on this device is mirrored to the outputs you enable below.",
            font=ctk.CTkFont(size=11),
            text_color=MUTED,
        ).pack(anchor="w", pady=(2, 0))

    def _build_devices_card(self) -> None:
        card = ctk.CTkFrame(self.body, fg_color=CARD, corner_radius=14)
        card.pack(fill="both", expand=True, padx=22, pady=6)
        head = ctk.CTkFrame(card, fg_color="transparent")
        head.pack(fill="x", padx=16, pady=(14, 4))
        ctk.CTkLabel(
            head, text="OUTPUT DEVICES", font=ctk.CTkFont(size=11, weight="bold"), text_color=MUTED
        ).pack(side="left")
        self.refresh_btn = ctk.CTkButton(
            head, text="Refresh", width=90, height=28, corner_radius=8,
            fg_color=CARD_ALT, hover_color="#2a2f3d", text_color=TEXT, font=ctk.CTkFont(size=12),
            command=self.refresh_devices,
        )
        self.refresh_btn.pack(side="right")
        self.devices_frame = ctk.CTkScrollableFrame(card, fg_color="transparent")
        self.devices_frame.pack(fill="both", expand=True, padx=10, pady=(0, 12))

    def _build_footer(self) -> None:
        footer = ctk.CTkFrame(self.body, fg_color="transparent")
        footer.pack(fill="x", padx=22, pady=(6, 20))
        self.status_label = ctk.CTkLabel(
            footer, text="Stopped", font=ctk.CTkFont(size=13), text_color=MUTED
        )
        self.status_label.pack(side="left")
        self.start_btn = ctk.CTkButton(
            footer, text="Start sharing", width=170, height=46, corner_radius=12,
            font=ctk.CTkFont(size=15, weight="bold"), fg_color=ACCENT, hover_color=ACCENT_HOVER,
            command=self.toggle_start,
        )
        self.start_btn.pack(side="right")

    def _clear_devices(self) -> None:
        for row in self.rows:
            row.destroy()
        self.rows = []
        for widget in self.devices_frame.winfo_children():
            widget.destroy()

    def refresh_devices(self, first: bool = False) -> None:
        was_running = self.engine.running
        if was_running:
            self.engine.stop()
        for job in self._delay_jobs.values():
            try:
                self.after_cancel(job)
            except Exception:
                pass
        self._delay_jobs.clear()
        self._clear_devices()
        self.engine.devices = {}

        self.source_mics = {}
        try:
            for mic in all_loopback_sources():
                try:
                    label = safe_name(mic, f"Output {len(self.source_mics) + 1}")
                except Exception:
                    continue
                self.source_mics[label] = mic
            default_mic = default_loopback()
            self.engine.source_mic = None
            self.source_menu.configure(values=["System default"] + list(self.source_mics.keys()))
            self.source_menu.set("System default")
            self.source_label.configure(text=f"Capturing: {safe_name(default_mic)}")
            source_ok = True
        except Exception as exc:
            self.source_label.configure(text=f"System-audio loopback unavailable ({exc})")
            source_ok = False

        devices, idx = list_output_devices()
        default_idx = default_output_index(idx)
        saved = self.settings.get("devices", {})

        count = 0
        for dev in devices:
            if dev["index"] == default_idx:
                continue
            state = Device(dev["index"], pretty_device_name(dev["name"], dev["index"]))
            preset = saved.get(dev["name"], {})
            state.volume = float(preset.get("volume", 1.0))
            state.delay_ms = int(preset.get("delay_ms", 0))
            state.enabled = bool(preset.get("enabled", False))
            self.engine.devices[state.name] = state
            self.rows.append(DeviceRow(self, self.devices_frame, state))
            count += 1

        if count == 0:
            ctk.CTkLabel(
                self.devices_frame,
                text="No additional output devices found.\nConnect a headset or speaker, then press Refresh.",
                text_color=MUTED, font=ctk.CTkFont(size=13), justify="left",
            ).pack(anchor="w", padx=10, pady=20)

        if not source_ok:
            self._set_status("System-audio loopback unavailable", BAD)
        elif was_running and not first:
            self._set_status("Devices refreshed - stopped. Press Start sharing.", MUTED)

    def persist_settings(self) -> None:
        if self._save_job is not None:
            try:
                self.after_cancel(self._save_job)
            except Exception:
                pass
        self._save_job = self.after(700, self._flush_settings)

    def _flush_settings(self) -> None:
        self._save_job = None
        data = {"devices": {}}
        for name, dev in self.engine.devices.items():
            data["devices"][name] = {
                "enabled": dev.enabled,
                "volume": round(dev.volume, 3),
                "delay_ms": dev.delay_ms,
            }
        self.settings = data
        save_settings(data)

    def schedule_delay(self, dev: Device) -> None:
        job = self._delay_jobs.get(dev.name)
        if job is not None:
            try:
                self.after_cancel(job)
            except Exception:
                pass
        self._delay_jobs[dev.name] = self.after(350, lambda d=dev: self._apply_delay(d))

    def _apply_delay(self, dev: Device) -> None:
        self._delay_jobs.pop(dev.name, None)
        self.engine.set_delay(dev, dev.delay_ms)

    def _on_source_change(self, value: str) -> None:
        if value == "System default":
            self.engine.source_mic = None
            try:
                self.source_label.configure(text=f"Capturing: {safe_name(default_loopback())}")
            except Exception:
                self.source_label.configure(text="Capturing: system default")
        else:
            self.engine.source_mic = self.source_mics.get(value)
            self.source_label.configure(text=f"Capturing: {value}")
        if self.engine.running:
            self.engine.stop()
            self.start_sharing()

    def toggle_start(self) -> None:
        if self._sharing:
            self.stop_sharing()
        else:
            self.start_sharing()

    def start_sharing(self) -> None:
        try:
            self.engine.start()
        except Exception as exc:
            self._sharing = False
            self._set_status(f"Cannot start: {exc}", BAD)
            return
        self._sharing = True
        self._silent_since = None
        self.start_btn.configure(text="Stop sharing", fg_color=BAD, hover_color="#dc2626")
        self._set_status(f"Sharing to {len(self.engine.active)} device(s)", GOOD)
        self.persist_settings()
        self._update_tray()

    def stop_sharing(self) -> None:
        self._sharing = False
        self.engine.stop()
        self.start_btn.configure(text="Start sharing", fg_color=ACCENT, hover_color=ACCENT_HOVER)
        self._set_status("Stopped", MUTED)
        self._update_tray()

    def _set_status(self, text: str, color: str) -> None:
        self.status_label.configure(text=text, text_color=color)

    def _on_configure(self, event) -> None:
        if event.widget is not self:
            return
        self._resizing = True
        if self._body_job is not None:
            try:
                self.after_cancel(self._body_job)
            except Exception:
                pass
        self._body_job = self.after(110, self._apply_body_size)

    def _apply_body_size(self) -> None:
        self._body_job = None
        self._resizing = False
        self._last_level = -1.0
        try:
            self.body.place_configure(
                width=max(1, self.winfo_width()), height=max(1, self.winfo_height())
            )
        except Exception:
            pass

    def _tick(self) -> None:
        if self._resizing:
            self._tick_job = self.after(200, self._tick)
            return

        running = self.engine.running
        if self._sharing and not running:
            self._sharing = False
            self.start_btn.configure(text="Start sharing", fg_color=ACCENT, hover_color=ACCENT_HOVER)
            if self.engine.capture_error:
                self._set_status(f"Capture stopped: {self.engine.capture_error}", BAD)
            else:
                self._set_status("Stopped", MUTED)
            self._update_tray()

        active = {d.name for d in self.engine.active}
        for row in self.rows:
            dev = row.dev
            if dev.error:
                row.set_state("error", BAD)
            elif running and dev.name in active:
                row.set_state("live", GOOD)
            elif dev.enabled and running:
                row.set_state("starting", MUTED)
            else:
                row.set_state("", MUTED)

        peak = self.engine.last_peak if running else 0.0
        if abs(peak - self._last_level) > 0.003:
            self._last_level = peak
            self.level_bar.set(min(1.0, peak))
            if peak > 1e-4:
                self.level_label.configure(
                    text=f"input: {20 * math.log10(peak):.0f} dB", text_color=GOOD
                )
            else:
                self.level_label.configure(text="input: -inf dB", text_color=MUTED)
        if peak > 1e-4:
            self._silent_since = None
        elif running:
            if self._silent_since is None:
                self._silent_since = time.time()
            elif time.time() - self._silent_since > 1.5 and not self.engine.capture_error:
                self._set_status(
                    "No audio on the source. Play something, or pick another source above.", BAD
                )
        else:
            self._silent_since = None

        self._tick_job = self.after(400, self._tick)

    def _log(self, message: str) -> None:
        try:
            print(message)
        except Exception:
            pass

    def hide_to_tray(self) -> None:
        if self.icon is None:
            self.quit_app()
            return
        self.withdraw()

    def _setup_tray(self) -> None:
        try:
            image = make_icon_image(64)
            menu = pystray.Menu(
                pystray.MenuItem("Show window", self._tray_call(self.show_window), default=True),
                pystray.MenuItem("Start sharing", self._tray_call(self.start_sharing)),
                pystray.MenuItem("Stop sharing", self._tray_call(self.stop_sharing)),
                pystray.Menu.SEPARATOR,
                pystray.MenuItem("Quit", self._tray_call(self.quit_app)),
            )
            self.icon = pystray.Icon(APP_ID, image, APP_NAME, menu)
            threading.Thread(target=self.icon.run, name="tray", daemon=True).start()
        except Exception as exc:
            self._log(f"Tray unavailable: {exc}")
            self.icon = None

    def _tray_call(self, func):
        def handler(*_args):
            self.after(0, func)

        return handler

    def _update_tray(self) -> None:
        if self.icon is not None:
            try:
                self.icon.title = f"{APP_NAME} - {'sharing' if self.engine.running else 'idle'}"
            except Exception:
                pass

    def show_window(self) -> None:
        self.deiconify()
        self.lift()
        self.focus_force()

    def quit_app(self) -> None:
        if self._quitting:
            return
        self._quitting = True
        try:
            if self._tick_job is not None:
                self.after_cancel(self._tick_job)
                self._tick_job = None
            if self._body_job is not None:
                self.after_cancel(self._body_job)
                self._body_job = None
        except Exception:
            pass
        try:
            if self._save_job is not None:
                self.after_cancel(self._save_job)
                self._save_job = None
            self._flush_settings()
        except Exception:
            pass
        try:
            self.engine.stop()
        except Exception:
            pass
        if self.icon is not None:
            try:
                self.icon.stop()
            except Exception:
                pass
        try:
            self.destroy()
        except Exception:
            pass


def run_selftest() -> None:
    os.makedirs(CONFIG_DIR, exist_ok=True)
    ok_path = os.path.join(CONFIG_DIR, "selftest.ok")
    err_path = os.path.join(CONFIG_DIR, "selftest.err")
    for path in (ok_path, err_path):
        try:
            os.remove(path)
        except OSError:
            pass
    try:
        import soundcard  # noqa: F401
        import sounddevice  # noqa: F401
        import customtkinter  # noqa: F401
        import pystray  # noqa: F401
        from PIL import Image  # noqa: F401

        np.random.default_rng(0).random(4)
        make_icon_image(64)
        with open(ok_path, "w", encoding="utf-8") as handle:
            handle.write(__version__)
    except Exception:
        import traceback

        with open(err_path, "w", encoding="utf-8") as handle:
            handle.write(traceback.format_exc())
        raise SystemExit(1)


def main() -> None:
    enable_dpi_awareness()
    if "--selftest" in sys.argv:
        run_selftest()
        return
    try:
        app = App()
        app.mainloop()
    except Exception:
        import traceback

        detail = traceback.format_exc()
        try:
            os.makedirs(CONFIG_DIR, exist_ok=True)
            with open(os.path.join(CONFIG_DIR, "error.log"), "w", encoding="utf-8") as handle:
                handle.write(detail)
        except Exception:
            pass
        try:
            from tkinter import messagebox

            messagebox.showerror(APP_NAME, f"An unexpected error occurred:\n\n{detail}")
        except Exception:
            pass


if __name__ == "__main__":
    main()
