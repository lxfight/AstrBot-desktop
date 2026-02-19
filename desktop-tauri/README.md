# AstrBot Desktop (Tauri)

该目录是 `AstrBot-desktop` 仓库中的 Tauri 桌面壳实现。

注意：

- 上游源码仓库与分支通过环境变量配置，不在脚本中写死：
  - `ASTRBOT_SOURCE_GIT_URL`
  - `ASTRBOT_SOURCE_GIT_REF`
- 原始项目仓库：`https://github.com/AstrBotDevs/AstrBot`
- `zouyonghe/AstrBot@cpython-runtime-refactor` 仅作为当前临时测试上游。
- 也可通过 `ASTRBOT_SOURCE_DIR` 直接指定本地 AstrBot 源码目录（优先级最高）。
- Tauri 壳本身在本目录维护，不直接改造上游仓库结构。

`ASTRBOT_SOURCE_GIT_URL` 支持两种格式：

- `https://github.com/AstrBotDevs/AstrBot.git`
- `https://github.com/AstrBotDevs/AstrBot/tree/main`（会自动解析分支）

## 行为设计

与 PR #5170 的 CPython 方案保持一致：

- 打包模式：读取 `runtime-manifest.json`，使用内置 `CPython + launch_backend.py` 启动后端
- 开发模式：默认执行 `uv run main.py`
- 启动阶段统一注入 UTF-8 相关环境变量，避免 Windows 编码问题

## 版本与升级

- 桌面端版本号在构建前自动同步为上游 `pyproject.toml` 的 `[project].version`。
- 同步入口：`pnpm --dir desktop-tauri run sync:version`。
- `prepare:webui` / `prepare:backend` / `prepare:resources` 都会先执行版本同步。
- 当前升级方式是发布新安装包后覆盖安装（`.dmg` / `.msi` / `.deb` 等）。
- 当前未启用 Tauri 内置 updater（即暂不支持应用内自更新）。

## 前置依赖

- Rust toolchain
- Node.js + `pnpm`
- Tauri CLI 2.10.0（`cargo tauri -V`）
- `uv`
- CPython runtime 目录（打包时需要，推荐 `python-build-standalone install_only`）

## 开发运行

在仓库根目录执行：

```bash
export ASTRBOT_SOURCE_GIT_URL=https://github.com/AstrBotDevs/AstrBot.git
export ASTRBOT_SOURCE_GIT_REF=main
pnpm --dir desktop-tauri install
pnpm --dir desktop-tauri run dev
```

## 打包构建

直接执行：

```bash
export ASTRBOT_SOURCE_GIT_URL=https://github.com/AstrBotDevs/AstrBot.git
export ASTRBOT_SOURCE_GIT_REF=main
pnpm --dir desktop-tauri install
pnpm --dir desktop-tauri run build
```

`build` 会按顺序执行：

1. 自动解析（或克隆）AstrBot 源码
2. 构建 dashboard 并同步 WebUI 资源
3. 自动下载并缓存 CPython runtime 到 `desktop-tauri/runtime/`（若本地没有）
4. 使用本仓库内置 `scripts/backend/build-backend.mjs` 生成后端 runtime 目录
5. 执行 `cargo tauri build`（由 `pnpm --dir desktop-tauri run build` 调用）

可选：使用本地源码目录

```bash
export ASTRBOT_SOURCE_DIR=/path/to/AstrBot
pnpm --dir desktop-tauri run build
```

如果不设置 `ASTRBOT_SOURCE_DIR`，将使用你配置的 git 上游参数：

```bash
export ASTRBOT_SOURCE_GIT_URL=https://github.com/AstrBotDevs/AstrBot.git
export ASTRBOT_SOURCE_GIT_REF=main
```

> 注意：首次执行会自动克隆到 `desktop-tauri/vendor/AstrBot`；后续会自动 fetch 并 checkout 到指定分支。

当前临时测试上游（官方仓库尚未合入对应代码）：

```bash
export ASTRBOT_SOURCE_GIT_URL=https://github.com/zouyonghe/AstrBot.git
export ASTRBOT_SOURCE_GIT_REF=cpython-runtime-refactor
```

可选：使用你自己的 CPython runtime（会覆盖自动下载）

```bash
export ASTRBOT_DESKTOP_CPYTHON_HOME=/path/to/cpython-runtime
pnpm --dir desktop-tauri run build
```

## 兼容环境变量

- `ASTRBOT_BACKEND_URL`
- `ASTRBOT_BACKEND_AUTO_START`
- `ASTRBOT_BACKEND_TIMEOUT_MS`
- `ASTRBOT_BACKEND_CMD`
- `ASTRBOT_BACKEND_CWD`
- `ASTRBOT_ROOT`
- `ASTRBOT_WEBUI_DIR`
- `ASTRBOT_SOURCE_DIR`
- `ASTRBOT_SOURCE_GIT_URL`
- `ASTRBOT_SOURCE_GIT_REF`
- `ASTRBOT_PBS_RELEASE`
- `ASTRBOT_PBS_VERSION`

打包模式自动注入：

- `ASTRBOT_ELECTRON_CLIENT=1`（复用现有后端打包运行逻辑）
- `PYTHONUTF8=1`
- `PYTHONIOENCODING=utf-8`
