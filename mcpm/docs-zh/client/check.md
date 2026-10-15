# mcpm client check

检查版本声明的库、natives 和资源完整性，不修改文件。

```sh
mcpm client check [VERSION ...] [--directory PATH] [--all]
mcpm client check 1.18.1 "My Modpack" --directory "$HOME/Downloads/.minecraft"
mcpm client check --directory "$HOME/Downloads/.minecraft"
mcpm client check --all --directory "$HOME/Downloads/.minecraft"
mcpm --raw client check --all --libraries hash --assets skip --asset-index skip --version-jar skip > checks.tsv
```

版本 ID 是 `client list` 中显示的版本目录名。支持多个 ID，按传入顺序检查并去重。`--all` 检查全部版本，包括 BrokenVersion，不能与显式 ID 同时使用。

不传 ID 和 `--all` 时，在标准错误终端中打开选择界面：上下键选择一个版本，Enter 确认，Esc 或 Ctrl-C 取消。取消返回成功。交互界面需要标准输入和标准错误均连接终端，`--raw` 会禁用交互；非交互环境需显式指定 ID 或 `--all`。

`--directory`（`-d`）指定 Minecraft 根目录。未传时选取当前路径中最后一个目录名恰好为 `.minecraft` 的目录；不存在则使用当前目录。自动识别时提示可用 `--directory .` 强制使用当前目录，`--raw` 隐藏提示。

四个选项均接受 `size`、`hash` 或 `skip`：`--libraries` 默认 `hash`，`--assets` 默认 `size`，`--asset-index` 默认 `hash`，`--version-jar` 默认 `hash`。`size` 只读取文件元数据检查大小；`hash` 先检查声明的大小，再检查 SHA-1，大小不符时不计算哈希；`skip` 跳过该项。缺少对应大小或哈希元数据时标记为未验证。跳过索引检查但启用资源对象检查时，仍读取并解析索引。游戏 JAR 支持继承版本和显式 `jar` 别名。

前端根据当前系统、架构和系统版本构造 `CheckRules`，仅检查适用的库和 natives，可选启动 features 不启用。检查结果不代表 Mod、Java 完整或游戏能够启动。

使用 `--strict` 将全部四类检查设为 `hash`，使用 `--shallow` 将全部设为 `size`。两者互斥，并且均不能与 `--libraries`、`--assets`、`--asset-index`、`--version-jar` 混用，无论参数顺序如何。

标准输出和标准错误均为终端时，使用四行进度界面：

```text
Step 3/5 Checking libraries (version)
[Main Progress Bar]
23/5000 [libraries/example.jar]
[Step Progress Bar]
```

检查多个版本时，最上方额外显示一条总进度，包含已完成版本数、总版本数和当前版本名；报告打印在进度区上方。默认共有五步：准备、游戏 JAR、库、资源索引、资源对象，跳过的类别不计入步骤总数。

主进度表示步骤进度，步骤进度表示已完成检查的文件数量，包含失败和未验证的检查。文件名表示当前即将检查的文件，步骤结束时保留最后检查的文件名。文件读取和哈希计算期间不发送进度事件，也不显示字节进度。资源对象总数只有在索引通过验证并解析后才确定；索引失败时总数保持未知。

标题和进度条使用亮青主题色，数量和文件名亮白，完整结果亮绿，失败亮红，未验证项和原因亮黄。`NO_COLOR` 保留布局但关闭颜色；`--raw` 或重定向输出禁用进度，使用五列 TSV：版本、状态、检查目标、路径、详细信息。每个版本先输出 `summary` 行，再输出问题和未验证文件。控制字符会被转义，诊断写入标准错误。

全部选中检查项通过时返回 `0`；`--all` 无版本或用户取消选择也返回 `0`。检查失败、缺少所需验证元数据、输入输出错误或无法交互选择时返回 `1`；参数错误返回 `2`。多个版本会全部检查后再汇总未通过的数量。
