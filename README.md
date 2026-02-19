# AstrBot-desktop

该仓库用于桌面端实验与构建，不直接改动上游 `AstrBot` 项目代码。

## 目录说明

- `desktop-tauri/`: 基于 Tauri 的桌面壳实现（本仓库主要改造目标）

## Tauri 桌面端

请参考：`desktop-tauri/README.md`

原始项目仓库：

- `https://github.com/AstrBotDevs/AstrBot`

当前上游源码建议通过环境变量指定（默认示例）：

- `ASTRBOT_SOURCE_GIT_URL=https://github.com/AstrBotDevs/AstrBot.git`

临时测试（官方仓库尚未合入相关代码）可覆盖为：

- `ASTRBOT_SOURCE_GIT_URL=https://github.com/zouyonghe/AstrBot.git`
- `ASTRBOT_SOURCE_GIT_REF=cpython-runtime-refactor`

也可直接传：

- `ASTRBOT_SOURCE_GIT_URL=https://github.com/zouyonghe/AstrBot/tree/cpython-runtime-refactor`

`desktop-tauri` 会在本仓库内自动缓存 CPython runtime（`desktop-tauri/runtime/`），
不依赖上游仓库的桌面构建脚本。

桌面端版本号会在资源准备阶段自动同步为上游 `pyproject.toml` 的 `[project].version`。
