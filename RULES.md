<!-- 本文件由 xtask 自动生成：`cargo run -p xtask -- gen-rules`。请勿手改；改规则请改 crates/core/src/*_rules.toml，再重跑生成。 -->

# 清理规则透明度

本页把编译进 `mc` 二进制的**全部清理规则**逐条列出——路径模式、安全等级、删除影响、恢复方式、项目根守卫、分类。目的是让任何人无需读代码即可审计「这个工具到底会动哪些文件、为什么安全」。内容由源规则表机械投影而成，与二进制行为一一对应（CI 有漂移门禁保证同步）。

安全等级语义（详见 `crates/core/src/models.rs` 的 `SafetyLevel` 文档注释）：

- **Safe**：零数据丢失，下次需要时自动透明补回（共享/下载缓存、IDE 索引等）。默认勾选。
- **Moderate**：零数据丢失，但需用户主动重建一次（`node_modules`、`target`、`DerivedData` 等）。默认勾选。
- **Risky**：可能丢失不可再生数据或有价值状态（Docker 命名卷、Xcode Archives、装好环境的 AVD）。默认不勾选，删除需在 TUI 输入 `delete` 二次确认。

> 合计 **26** 条规则：Safe 17 · Moderate 6 · Risky 3。删除默认移入废纸篓（可恢复）。安全边界见 [SECURITY.md](SECURITY.md)。

## 系统缓存与日志（Clean）

`mc clean` 使用的规则：按精确路径匹配系统缓存、日志、临时文件。

| 规则 | 路径模式 | 安全等级 | 默认预选 | 影响 | 恢复 | 项目根守卫 | 分类 |
| --- | --- | --- | :---: | --- | --- | --- | --- |
| System Caches | `~/Library/Caches` | Safe | 是 | 应用缓存被清空，下次使用时自动重建；极少数应用可能在此存放非缓存数据 | 无需操作，应用会按需自动重新生成 | — | 系统缓存 |
| Application Logs | `~/Library/Logs` | Safe | 是 | 历史日志被删除，不影响应用运行，仅丢失过往诊断记录 | 无需操作，应用会继续写入新日志 | — | 系统缓存 |
| System Temp Files | `/tmp` | Safe | 是 | 临时文件被删除，正在运行的程序会按需重新生成 | 无需操作，系统按需重建 | — | 系统缓存 |
| Chrome Cache | `~/Library/Caches/Google/Chrome` | Safe | 是 | 网页缓存被清空，首次访问网站会重新下载资源、略慢几秒；不影响书签/密码/历史 | 无需操作，浏览时自动重建 | — | 浏览器缓存 |
| Safari Cache | `~/Library/Caches/com.apple.Safari` | Safe | 是 | 网页缓存被清空，首次访问网站会重新下载资源、略慢几秒；不影响书签/密码/历史 | 无需操作，浏览时自动重建 | — | 浏览器缓存 |
| Firefox Cache | `~/Library/Caches/Firefox` | Safe | 是 | 网页缓存被清空，首次访问网站会重新下载资源、略慢几秒；不影响书签/密码/历史 | 无需操作，浏览时自动重建 | — | 浏览器缓存 |

## 开发产物（Purge）

`mc purge <dir>` 使用的规则：按目录名剪枝匹配开发依赖与构建产物，命中需满足「项目根守卫」以消除误报。

| 规则 | 路径模式 | 安全等级 | 默认预选 | 影响 | 恢复 | 项目根守卫 | 分类 |
| --- | --- | --- | :---: | --- | --- | --- | --- |
| node_modules | `node_modules/` | Moderate | 是 | 该项目的依赖被清空，下次构建/运行前需重新安装 | 在项目目录运行 npm/pnpm/yarn install | 旁有 `package.json` | Node.js |
| Rust target | `target/` | Moderate | 是 | 该项目的编译产物被清空，下次构建为全量冷编译、较慢 | 在项目目录运行 cargo build | 旁有 `Cargo.toml` | Rust |
| Python venv | `.venv/`<br>`venv/` | Moderate | 是 | 该虚拟环境被删除，需重建并重新安装依赖 | python -m venv 重建后 pip install -r requirements.txt | 内含 `pyvenv.cfg` | Python |
| __pycache__ | `__pycache__/` | Safe | 是 | 字节码缓存被删除，下次运行时自动重新生成、无感 | 无需操作，运行时自动重建 | — | Python |
| dist/build | `dist/`<br>`build/` | Moderate | 否 | 构建输出被清空；注意 electron-builder 等以 build/ 存放图标/授权文件等手工资源 | 重新运行项目的构建命令；手工资源若被误删需从版本库恢复 | 旁有 `package.json` | Build Output |
| Gradle Cache | `~/.gradle/caches` | Safe | 是 | Gradle 依赖/构建缓存被清空，下次构建按需重新下载；不影响 gradle.properties 等配置与密钥 | 无需操作，下次构建自动重新下载 | — | Gradle |
| DerivedData | `~/Library/Developer/Xcode/DerivedData` | Moderate | 是 | Xcode 项目构建缓存被清空，下次构建为全量冷编译、耗时较长 | 在 Xcode 重新构建项目（Cmd+B），首次为全量冷编译 | — | Xcode |
| Pods | `Pods/` | Moderate | 是 | 该项目的 CocoaPods 依赖被清空，需重新安装 | 在项目目录运行 pod install | 旁有 `Podfile` | CocoaPods |
| Docker Desktop Data | `~/Library/Containers/com.docker.docker/Data/vms` | Risky | 是 | 删除后全部本地镜像、容器和命名卷丢失，命名卷内的数据（如数据库）不可恢复 | 镜像可重新 pull/build；命名卷内数据无法恢复 | — | Docker |
| Docker buildx Cache | `~/.docker/buildx` | Safe | 是 | buildx 构建缓存被清空，下次构建按需重建；不影响镜像/容器/卷 | 无需操作，下次构建自动重建 | — | Docker |
| Maven Repository | `~/.m2/repository` | Safe | 是 | 已下载的依赖被清空，下次构建按需重新下载 | 无需操作，下次构建自动重新下载 | — | Java |
| Homebrew Cache | `~/Library/Caches/Homebrew` | Safe | 是 | 已下载的安装包缓存被清空，下次安装/升级按需重新下载 | 无需操作，下次 brew 操作自动重新下载 | — | Homebrew |
| Go Module Cache | `~/go/pkg/mod` | Safe | 是 | 已下载的模块缓存被清空，下次构建按需重新下载 | 无需操作，下次 go build 自动重新下载 | — | Go |
| Cargo Cache | `~/.cargo/registry`<br>`~/.cargo/git` | Safe | 是 | 已下载的 crate 缓存被清空，下次构建按需重新下载 | 无需操作，下次 cargo build 自动重新下载 | — | Rust |
| npm/pnpm/yarn Cache | `~/.npm/_cacache`<br>`~/Library/pnpm/store`<br>`~/.yarn/cache` | Safe | 是 | 包管理器全局缓存被清空，下次安装按需重新下载 | 无需操作，下次 install 自动重新下载 | — | Node.js |
| pip Cache | `~/Library/Caches/pip` | Safe | 是 | pip 下载缓存被清空，下次安装按需重新下载 | 无需操作，下次 pip install 自动重新下载 | — | Python |
| Xcode Archives | `~/Library/Developer/Xcode/Archives` | Risky | 是 | 归档含已发布 App 的 dSYM，删除后无法再符号化线上崩溃日志，不可再生 | 不可恢复（除非保留了对应构建的 dSYM 备份） | — | Xcode |
| Android AVD | `~/.android/avd` | Risky | 是 | 模拟器镜像被删除，其中已安装的应用与配置状态丢失，需重建并重新配置 | 在 Android Studio 重新创建 AVD 并重装应用/环境 | — | Android |
| Android SDK Temp | `~/Library/Android/sdk/.temp` | Safe | 是 | SDK 临时文件被删除，不影响已安装的 SDK 组件 | 无需操作，SDK 操作时按需重建 | — | Android |
| JetBrains Cache | `~/Library/Caches/JetBrains` | Safe | 是 | IDE 缓存与索引被清空，下次启动会重建索引、首次略慢；不影响项目与配置 | 无需操作，IDE 启动时自动重建索引 | — | JetBrains |
