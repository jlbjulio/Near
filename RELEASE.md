## Near 1.0.5

A quiet AirPods companion for Windows, written in Rust.

- A silent low battery flyout appears at 10%, using fresh observations.
- The warning fades away without taking focus from your current app.
- Removed the 20% reminder and Windows battery notifications. The AirPods' built-in sound is unchanged.
- Left, right, and case warnings are grouped so one does not overwrite another.
- Charging and recovered batteries rearm alerts; missing or old readings do not trigger them.
- Re-enabling battery alerts works immediately with a fresh low reading.
- Existing settings, ear-sensor fixes, and Sonar audio routing are preserved.

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

Download **Near-1.0.5-windows-x64.zip**, extract it, and run **Near.exe**.

**Compatibility:** Windows 10 version 2004 or later / Windows 11, x64, with Bluetooth LE. First-time pairing uses Windows Bluetooth settings. AirPods battery broadcasts are approximate, in 10% steps; case readings update while the case broadcasts. Ear sensing varies by model and firmware. AirPods Pro 3 and AirPods Max detection is included; their ear sensing is not advertised as supported. Real-device testing across every model is still outstanding.

Near runs locally. It has no account, subscription, telemetry, or custom driver.

Battery is available in the flyout and system tray. Windows widgets and live tiles are not included in this release.
