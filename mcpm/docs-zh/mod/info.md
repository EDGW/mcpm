# `mcpm mod info`

```text
mcpm mod info [OPTIONS] <FILE>...
```

按照参数顺序检查一个或多个 `.jar` 或 `.jar.disabled` 文件，不接受目录。输出包括外层文件路径、根据文件名推断的启用状态、Loader 记录、Mod 名称、ID、版本和依赖声明，也包含已识别的嵌套 Mod。

```sh
mcpm mod info example.jar
mcpm mod info example.jar another.jar.disabled --loaders fabric,quilt
mcpm --raw mod info example.jar > info.txt
```

未指定 Loader 筛选时，所有 Loader 都无法识别的文件视为错误；指定筛选时，此类文件会被跳过。Loader 识别错误写入标准错误，成功的记录仍会显示，但命令返回失败状态。后续文件继续处理。

通用选项见 [Loader 选择](README.md#loader-选择)，颜色、忽略文件提示和 `--raw` 行为见[终端输出](../README.md#终端输出)。

[命令参考](../README.md) · [mod 概览](README.md) · [English](../../docs/mod/info.md)
