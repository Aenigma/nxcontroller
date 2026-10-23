"""Python client for the nxcontroller ESP32-S3 firmware."""

from .client import Controller, NXError
from .discovery import DeviceInfo, discover
from .inputs import Button, Hat, Key, Modifier

__all__ = ["Button", "Controller", "DeviceInfo", "Hat", "Key", "Modifier", "NXError", "discover"]
__version__ = "0.1.0"
