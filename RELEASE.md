## Near 1.0.4

A quiet AirPods companion for Windows, written in Rust.

- Settings update independently, preserving recent changes, selected headphones, speaker choice, and manual disconnections.
- Rapid switch clicks are processed in order without overwriting other settings.
- Enabling or disabling automatic pause clears the previous sensor baseline to avoid commands from an earlier setting.
- Failed settings saves show a message and retain the previous configuration.
- Expanded left/right battery, charging, case, primary-earbud, and single-earbud regression coverage.
- Includes the AirPods 4 ear-sensor and Bluetooth broadcast gap fixes from 1.0.3.

- Battery and charging indicators in a compact tray flyout.
- A case-opening popup that fades away automatically.
- Automatic reconnection when paired headphones are detected nearby.
- Disconnect / Connect in the flyout, with manual disconnection remembered across restarts.
- Consistent battery observations across private BLE addresses no longer trigger a false “multiple AirPods” warning.
- Battery messages wrap across two lines, with better spacing and display scaling.
- Automatic pause and resume when supported ear sensors report a change.
- Automatic switching to speakers when both earbuds report that they are removed.
- Audio routing that respects SteelSeries Sonar and other virtual mixers, with disconnection fallback only for directly selected AirPods.
- Playback controls, low battery alerts, and optional startup with Windows.
- A movable flyout that opens or focuses when you click the tray icon.
- Consistent Segoe UI Variable typography on Windows 11, vector playback controls, and a simple English interface.

Download **Near-1.0.4-windows-x64.zip**, extract it, and run **Near.exe**.

**Compatibility:** Windows 10 version 2004 or later / Windows 11, x64, with Bluetooth LE. First-time pairing uses Windows Bluetooth settings. AirPods battery broadcasts are approximate, in 10% steps; case readings update while the case broadcasts. Ear sensing varies by model and firmware. AirPods Pro 3 and AirPods Max detection is included; their ear sensing is not advertised as supported. Real-device testing across every model is still outstanding.

Near runs locally. It has no account, subscription, telemetry, or custom driver.

Battery is available in the flyout and system tray. Windows widgets and live tiles are not included in this release.
