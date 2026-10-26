# Forge · NeoForge 识别流程

识别逻辑位于 `mcpm-backend`，输入为静态解析得到的 `ServerLaunch`。整个过程不执行脚本、Java 或安装器，也不下载依赖。

- `Ok(Some(identity))`：识别成功，返回 Minecraft 版本和 loader 版本。
- `Ok(None)`：当前检测器不处理该入口或家族。
- `Err`：必要证据缺失、冲突或无法读取。上层跳过当前 launch，保留其他已识别候选。

## Forge / NeoForge 通用分发

![Forge 与 NeoForge 两个独立入口，以及后续的 FML 家族判断、启动声明检查和版本识别流程](diagrams/forge-neoforge-dispatch.svg)

相关实现：

- [`forge::server::identify`](../../src/local/modloader/impls/fml/forge/server.rs)：Forge 入口，单独分流 Shim。
- [`neoforge::server::identify`](../../src/local/modloader/impls/fml/neoforge/server.rs)：NeoForge 入口，直接调用共享 FML 检测。
- [`fml::server::identify` / `ensure_server_target`](../../src/local/modloader/impls/fml/server.rs)：家族选择、版本提取和已知客户端 target 排除。
- [`ServerLaunch::option`](../../src/local/mc/server/launch/mod.rs)：支持 `--name value` 和 `--name=value`；重复相同值允许，缺失值、空白值、以 `-` 开头的值或重复冲突值报错。

## Forge Shim 识别流程

![Forge Shim 的内嵌参数读取、显式选项优先级、依赖列表回退与版本合并流程](diagrams/forge-shim-identify.svg)

- 每个选项优先使用显式启动参数，未提供时才使用内嵌 `Arguments` 中的默认值。
- 依赖列表中的 Forge 版本证据须相互一致；已声明的版本优先保留，不要求与列表版本相同。

相关实现：

- [`forge::server::identify_shim`](../../src/local/modloader/impls/fml/forge/server.rs)：读取 Shim 元数据并合并版本来源。
- [`coordinate` / `agree`](../../src/local/modloader/impls/utils/coordinate.rs)：提取完整 Maven 路径中的版本，校验非空及多个声明的一致性。

图中的读取、解析和选项查询失败均返回 `Err`；错误保留来源上下文。识别只提取身份与版本证据，不保证依赖完整或服务器可启动。
