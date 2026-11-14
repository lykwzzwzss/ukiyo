# ukiyo

基于 Tauri + React + Rust 构建的游戏启动器，支持本地代理、RSA 补丁、GIMI Mod 注入与 FPS 解锁。

## 致谢 / 开源项目引用

本项目在以下开源项目的基础上开发与整合：

- **[Cultivation](https://github.com/Grasscutters/Cultivation)**：启动器基础架构、代理与补丁框架
- **[GIMI (GI-Model-Importer)](https://github.com/SilentNightSound/GI-Model-Importer)**：3DMigoto 模型 / 贴图 Mod 注入支持
- **[genshin-fps-unlock](https://github.com/34736384/genshin-fps-unlock)**：FPS 帧率解锁支持（`UnlockerStub.dll`）

## 从源码编译

### 环境要求
- Node.js + Yarn
- Rust（`1.80.0`）
- Microsoft C++ Build Tools（MSVC `link.exe`）

### 构建步骤
```powershell
yarn install
yarn build
```

编译完成后，Release 产物位于 `target/release/ukiyo.exe`（或 `.cargo/config.toml` 指定的 `target-dir`）。
