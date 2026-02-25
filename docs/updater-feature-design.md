# AstrBot Desktop 自动更新功能设计文档

## 1. 概述

本文档描述基于 Tauri v2 Updater 插件的自动更新功能设计方案，参考了被关闭的 PR #58 的实现思路。

## 2. 技术选型

### 2.1 核心依赖

| 组件 | 版本/要求 | 说明 |
|------|----------|------|
| Tauri | 2.0+ | 桌面应用框架 |
| tauri-plugin-updater | 2.0+ | 官方更新插件 |
| tauri-plugin-dialog | 2.0+ | 对话框插件（用于更新提示） |
| Rust | 1.86+ | 最低 Rust 版本要求 |
| minisign | - | 更新包签名工具 |

### 2.2 更新模式

采用 **静态 JSON 文件** 模式，通过 GitHub Releases 分发更新清单：

```
GitHub Releases
└── latest.json (更新清单)
    ├── AppImage (Linux)
    ├── .deb/.rpm (Linux 安装包)
    ├── .app.tar.gz (macOS)
    └── .nsis.zip (Windows)
```

## 3. 架构设计

### 3.1 整体流程

```
┌─────────────────────────────────────────────────────────────────┐
│                        前端 (WebUI)                              │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐ │
│  │ 检查更新按钮 │  │ 更新对话框  │  │ 下载进度/安装状态显示   │ │
│  └─────────────┘  └─────────────┘  └─────────────────────────┘ │
│         │                │                      │               │
│         ▼                ▼                      ▼               │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │              window.astrbotAppUpdater                    │   │
│  │  - checkForAppUpdate()                                   │   │
│  │  - installAppUpdate()                                    │   │
│  └─────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
                              │
                              │ Tauri IPC (invoke)
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    Rust 后端 (Tauri)                             │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │  desktop_bridge_check_desktop_app_update()               │  │
│  │  desktop_bridge_install_desktop_app_update()             │  │
│  └──────────────────────────────────────────────────────────┘  │
│                              │                                   │
│                              ▼                                   │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │         tauri-plugin-updater                             │  │
│  │  - updater.check()                                       │  │
│  │  - update.download_and_install()                         │  │
│  └──────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
                              │
                              │ HTTPS
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                     GitHub Releases                            │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │  latest.json (更新清单)                                   │  │
│  │  *.sig (签名文件)                                        │  │
│  │  *.AppImage / *.deb / *.rpm / *.nsis.zip                 │  │
│  └──────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

### 3.2 核心模块

```
src-tauri/
├── Cargo.toml                    # 添加 updater/dialog 依赖
├── tauri.conf.json               # 更新器配置
├── capabilities/
│   └── default.json              # 权限配置
└── src/
    ├── main.rs                   # 插件注册
    ├── desktop_bridge_commands.rs # 新增更新相关命令
    └── app_types.rs              # 新增返回类型定义
```

## 4. 详细实现

### 4.1 依赖配置 (Cargo.toml)

```toml
[dependencies]
tauri = { version = "2.0", features = ["tray-icon"] }
tauri-plugin-updater = "2.0"
tauri-plugin-dialog = "2.0"
```

### 4.2 Tauri 配置 (tauri.conf.json)

```json
{
  "bundle": {
    "createUpdaterArtifacts": true
  },
  "plugins": {
    "updater": {
      "pubkey": "CONTENT_FROM_PUBLICKEY.PEM",
      "endpoints": [
        "https://github.com/AstrBotDevs/AstrBot-desktop/releases/latest/download/latest.json"
      ],
      "windows": {
        "installMode": "passive"
      }
    }
  }
}
```

### 4.3 权限配置 (capabilities/default.json)

```json
{
  "permissions": [
    "core:default",
    "updater:default",
    "dialog:default"
  ]
}
```

### 4.4 Rust 实现

#### 4.4.1 插件注册 (main.rs)

```rust
use tauri_plugin_updater::UpdaterExt;
use tauri_plugin_dialog::DialogExt;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::Builder::default())
        .invoke_handler(tauri::generate_handler![
            desktop_bridge_check_desktop_app_update,
            desktop_bridge_install_desktop_app_update,
            // ... 其他命令
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

#### 4.4.2 数据类型定义 (app_types.rs)

```rust
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesktopAppUpdateCheckResult {
    pub(crate) ok: bool,
    pub(crate) reason: Option<String>,
    pub(crate) current_version: String,
    pub(crate) latest_version: Option<String>,
    pub(crate) has_update: bool,
}
```

#### 4.4.3 检查更新命令

```rust
#[tauri::command]
pub(crate) async fn desktop_bridge_check_desktop_app_update(
    app_handle: AppHandle,
) -> DesktopAppUpdateCheckResult {
    let current_version = app_handle.package_info().version.to_string();

    let updater = match app_handle.updater() {
        Ok(updater) => updater,
        Err(error) => {
            return DesktopAppUpdateCheckResult {
                ok: false,
                reason: Some(format!("Failed to initialize updater: {error}")),
                current_version,
                latest_version: None,
                has_update: false,
            };
        }
    };

    match updater.check().await {
        Ok(Some(update)) => DesktopAppUpdateCheckResult {
            ok: true,
            reason: None,
            current_version,
            latest_version: Some(update.version.to_string()),
            has_update: true,
        },
        Ok(None) => DesktopAppUpdateCheckResult {
            ok: true,
            reason: None,
            current_version: current_version.clone(),
            latest_version: Some(current_version),
            has_update: false,
        },
        Err(error) => DesktopAppUpdateCheckResult {
            ok: false,
            reason: Some(format!("Failed to check update: {error}")),
            current_version,
            latest_version: None,
            has_update: false,
        },
    }
}
```

#### 4.4.4 安装更新命令

```rust
#[tauri::command]
pub(crate) async fn desktop_bridge_install_desktop_app_update(
    app_handle: AppHandle,
) -> BackendBridgeResult {
    let updater = match app_handle.updater() {
        Ok(updater) => updater,
        Err(error) => {
            return BackendBridgeResult {
                ok: false,
                reason: Some(format!("Failed to initialize updater: {error}")),
            };
        }
    };

    let update = match updater.check().await {
        Ok(Some(update)) => update,
        Ok(None) => {
            return BackendBridgeResult {
                ok: false,
                reason: Some("Already on latest version.".to_string()),
            };
        }
        Err(error) => {
            return BackendBridgeResult {
                ok: false,
                reason: Some(format!("Failed to check update: {error}")),
            };
        }
    };

    let target_version = update.version.to_string();

    if let Err(error) = update.download_and_install(|_, _| {}, || {}).await {
        return BackendBridgeResult {
            ok: false,
            reason: Some(format!("Failed to install update: {error}")),
        };
    }

    app_handle.request_restart();

    BackendBridgeResult {
        ok: true,
        reason: None,
    }
}
```

### 4.5 前端实现 (JavaScript)

#### 4.5.1 Bridge 注入 (bridge_bootstrap.js)

```javascript
window.astrbotAppUpdater = {
  checkForAppUpdate: () =>
    invokeBridge('desktop_bridge_check_desktop_app_update'),
  installAppUpdate: () =>
    invokeBridge('desktop_bridge_install_desktop_app_update'),
};
```

#### 4.5.2 前端调用示例

```javascript
// 检查更新
async function checkUpdate() {
  const result = await window.astrbotAppUpdater.checkForAppUpdate();

  if (!result.ok) {
    console.error('检查更新失败:', result.reason);
    return;
  }

  if (result.hasUpdate) {
    const confirmed = await confirm(
      `发现新版本 ${result.latestVersion}，是否立即更新？`,
      { title: '更新可用', kind: 'info' }
    );

    if (confirmed) {
      await installUpdate();
    }
  } else {
    await message('已是最新版本', { title: 'AstrBot', kind: 'info' });
  }
}

// 安装更新
async function installUpdate() {
  const result = await window.astrbotAppUpdater.installAppUpdate();

  if (result.ok) {
    await message('更新已安装，应用即将重启', { title: 'AstrBot', kind: 'info' });
  } else {
    await message(`更新失败：${result.reason}`, { title: 'AstrBot', kind: 'error' });
  }
}
```

## 5. 更新清单格式 (latest.json)

```json
{
  "version": "4.19.0",
  "notes": "修复了若干问题，提升了稳定性",
  "pub_date": "2026-02-25T12:00:00Z",
  "platforms": {
    "windows-x86_64": {
      "signature": "<.sig 文件内容>",
      "url": "https://github.com/AstrBotDevs/AstrBot-desktop/releases/download/v4.19.0/AstrBot_4.19.0_windows_x86_64_updater.zip"
    },
    "linux-x86_64": {
      "signature": "<.sig 文件内容>",
      "url": "https://github.com/AstrBotDevs/AstrBot-desktop/releases/download/v4.19.0/AstrBot_4.19.0_linux_x86_64_updater.tar.gz"
    },
    "macos-x86_64": {
      "signature": "<.sig 文件内容>",
      "url": "https://github.com/AstrBotDevs/AstrBot-desktop/releases/download/v4.19.0/AstrBot_4.19.0_macos_x86_64_updater.tar.gz"
    },
    "macos-aarch64": {
      "signature": "<.sig 文件内容>",
      "url": "https://github.com/AstrBotDevs/AstrBot-desktop/releases/download/v4.19.0/AstrBot_4.19.0_macos_aarch64_updater.tar.gz"
    }
  }
}
```

## 6. 密钥管理

### 6.1 生成密钥对

```bash
# 生成密钥对
cargo tauri signer generate -- -w ~/.tauri/astrbot.key

# 输出:
# - ~/.tauri/astrbot.key (私钥，保密)
# - ~/.tauri/astrbot.key.pub (公钥，放入 tauri.conf.json)
```

### 6.2 环境变量

```bash
# 构建时设置
export TAURI_SIGNING_PRIVATE_KEY="~/.tauri/astrbot.key"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
```

### 6.3 CI/CD 配置

在 GitHub Actions 中配置 Secrets：

```yaml
env:
  TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
  TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
```

## 7. 构建流程

### 7.1 本地构建

```bash
# 1. 生成密钥
make signing-key

# 2. 设置环境变量
export TAURI_SIGNING_PRIVATE_KEY_PATH="$HOME/.tauri/astrbot.key"
export ASTRBOT_UPDATER_PUBKEY_FILE="$HOME/.tauri/astrbot.key.pub"

# 3. 构建
make build-signed
```

### 7.2 CI 构建

```yaml
- name: Prepare Tauri build config
  uses: ./.github/actions/prepare-tauri-build-config
  with:
    updater-endpoint: ${{ env.ASTRBOT_UPDATER_ENDPOINT }}
    updater-pubkey: ${{ env.ASTRBOT_UPDATER_PUBKEY }}

- name: Build
  run: cargo tauri build --config "${ASTRBOT_TAURI_CONFIG_PATH}"
```

## 8. 用户交互流程

```
用户点击"检查更新"
        │
        ▼
┌─────────────────┐
│  显示"检查中..."  │
└─────────────────┘
        │
        ▼
  调用 checkDesktopAppUpdate()
        │
        ▼
┌─────────────────────────────────────┐
│           是否有更新？               │
└─────────────────────────────────────┘
        │
    ┌───┴───┐
    │       │
   是       否
    │       │
    ▼       ▼
┌─────────┐ ┌─────────────┐
│显示更新 │ │显示"已是最新"│
│对话框   │ │提示         │
└─────────┘ └─────────────┘
    │
    ▼
┌─────────────────┐
│  用户确认更新？  │
└─────────────────┘
    │
 ┌──┴──┐
 │     │
是     否
 │     │
 ▼     └──→ 结束
┌─────────────────┐
│  下载更新包      │
│  显示进度条      │
└─────────────────┘
    │
    ▼
┌─────────────────┐
│  安装并重启应用  │
└─────────────────┘
```

## 9. 错误处理

| 错误场景 | 处理方式 |
|---------|---------|
| 网络不可用 | 返回错误信息，提示用户检查网络 |
| 更新服务器无响应 | 尝试备用 endpoint（如配置多个） |
| 签名验证失败 | 拒绝安装，提示安全风险 |
| 磁盘空间不足 | 显示错误对话框，引导清理空间 |
| 更新包下载中断 | 支持断点续传 |
| 安装失败 | 回滚到旧版本（如可能） |

## 10. 平台差异

| 平台 | 更新包格式 | 安装模式 | 注意事项 |
|------|-----------|---------|---------|
| Windows | .nsis.zip | passive | 安装时应用自动退出 |
| macOS | .app.tar.gz | - | 需要用户授权 |
| Linux | .AppImage.tar.gz | - | 需要可执行权限 |

## 11. 实施步骤

### Phase 1: 基础环境搭建
- [ ] 添加 `tauri-plugin-updater` 和 `tauri-plugin-dialog` 依赖
- [ ] 生成签名密钥对
- [ ] 配置 `tauri.conf.json` 更新器插件
- [ ] 配置 `capabilities/default.json` 权限

### Phase 2: Rust 后端实现
- [ ] 在 `main.rs` 注册插件
- [ ] 在 `app_types.rs` 添加数据类型
- [ ] 在 `desktop_bridge_commands.rs` 实现更新命令
- [ ] 在 `bridge_bootstrap.js` 注入前端 API

### Phase 3: 前端 UI 实现
- [ ] 创建更新检查按钮组件
- [ ] 创建更新对话框组件
- [ ] 创建下载进度显示组件
- [ ] 集成到设置页面

### Phase 4: CI/CD 集成
- [ ] 配置 GitHub Actions Secrets
- [ ] 修改构建流程生成更新包
- [ ] 自动生成 `latest.json` 清单
- [ ] 上传更新包到 Releases

### Phase 5: 测试与发布
- [ ] 测试各平台更新流程
- [ ] 测试网络异常情况
- [ ] 测试签名验证
- [ ] 发布首个支持更新的版本

## 12. 参考资料

- [Tauri v2 Updater 官方文档](https://v2.tauri.app/plugin/updater/)
- [Tauri v2 Dialog 官方文档](https://v2.tauri.app/plugin/dialog/)
- [PR #58 - add signed updater artifact + manifest pipeline](https://github.com/AstrBotDevs/AstrBot-desktop/pull/58)
- [Tauri Updater GitHub 示例](https://github.com/tauri-apps/tauri-plugin-updater)
