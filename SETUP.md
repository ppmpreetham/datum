# Datum development setup (Windows)

## Windows prerequisites

Open PowerShell. Install Node.js, Rust, and the Microsoft C++ build tools.
The C++ workload is required for Tauri's Windows build.

```powershell
winget install --id OpenJS.NodeJS.LTS --exact
winget install --id Rustlang.Rustup --exact
winget install --id Microsoft.VisualStudio.2022.BuildTools --exact --override "--wait --passive --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

Accept the installer/UAC prompt when Windows displays it, then restart
PowerShell and install pnpm:

```powershell
npm install --global pnpm@11.25.0
```

Verify:

```powershell
cargo --version
pnpm --version
where.exe link
```

If `where.exe link` does not find the linker in a normal PowerShell, open the
Visual Studio **Developer PowerShell** and run the project there. The
installation must include MSVC and a Windows SDK.

From `A:\projects\datum`:

```powershell
pnpm install --frozen-lockfile
pnpm tauri dev
```

`pnpm-workspace.yaml` allows `esbuild`'s install script so this repo can
install with pnpm 11.

## Docker-assisted development

This mode runs the Vite frontend in Docker and the Tauri/Rust desktop app on
Windows. The desktop process must stay on Windows to open its window and access
Windows USB serial ports. Install Docker Desktop and start its Linux engine,
then install the Windows Rust/C++ prerequisites above. Node.js and pnpm are
also needed on Windows to run the Tauri CLI.

From `A:\projects\datum`, start the frontend in the first PowerShell window:

```powershell
docker compose up --build
```

Wait for Vite to report that it is ready at `http://localhost:1420`. In a
second PowerShell window, from the same folder, run:

```powershell
pnpm install --frozen-lockfile
cmd.exe /d /c 'call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat" >nul && pnpm tauri dev --config src-tauri/tauri.docker-dev.conf.json'
```

The `vcvars64.bat` step loads the C++ linker and Windows SDK paths for this
terminal. The extra Tauri config skips starting a second Vite process. Docker publishes
port 1420 only on the local computer. The Tauri window still hosts the
calibration commands; opening `http://localhost:1420` in an ordinary browser
shows the frontend but cannot call those commands. The current frontend has
not yet been wired to the calibration API described in `FRONTEND.md`.

To stop the frontend, press Ctrl+C in its PowerShell window, then run
`docker compose down`. After editing frontend files, run
`docker compose up --build` again to rebuild the image.

## WSL2 simulator

Install Ubuntu if `wsl --list --verbose` does not show it:

```powershell
wsl --install -d Ubuntu
```

Complete any Windows prompt or restart, then open Ubuntu and create its Linux
user. Datum remains a Windows/Tauri app. The simulator runs in Ubuntu/WSL2.
Use the official [ArduPilot Ubuntu build guide](https://ardupilot.org/dev/docs/building-setup-linux.html)
or [PX4 WSL2 setup guide](https://docs.px4.io/main/en/dev_setup/dev_env_windows_wsl.html)
to install a simulator. Each project has substantial build dependencies.

For ArduPilot Copter, after its official build setup:

```bash
cd ~/ardupilot/ArduCopter
../Tools/autotest/sim_vehicle.py --map --console
```

In MAVProxy, run `output` to see where it sends MAVLink. The standard setup
often uses UDP 14550/14551. Connect Datum with `udpin:127.0.0.1:14550` when
that port is forwarded to Windows. WSL2 network modes differ; follow the
[ArduPilot WSL networking instructions](https://ardupilot.org/dev/docs/sitl-on-windows-wsl.html)
if no heartbeat arrives. PX4 has its own [SITL instructions](https://docs.px4.io/main/en/simulation/),
including MAVLink connection settings.

## Validation sequence

1. Run `cargo test --lib` in `src-tauri` after the Windows linker is available.
2. Connect to ArduPilot and PX4 SITL separately. Confirm firmware detection,
   disarm status, sensor status, command ACK, failure, timeout, Stop, and
   reconnection behavior.
3. Test each calibration workflow against each firmware. Firmware versions may
   differ in available commands and progress messages; log any unsupported
   procedure instead of marking it complete.
4. Only after simulator verification, test with the intended real controller
   and propellers removed. Keep ESC and motor operations disabled until then.

No simulated run can verify physical gyro, accelerometer, compass, or radio
accuracy. A MAVLink `ACCEPTED` reply says the request was accepted; it alone
does not prove the sensor is calibrated.
