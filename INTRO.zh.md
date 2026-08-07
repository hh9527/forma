# 从可编程配置到可信计划：Forma 简介

配置很少会永远停留在“写下一棵数据树”。当系统开始复用配置、读取外部数据、
根据平台作选择、验证约束并生成命令时，它已经在执行一个程序。真正的问题不再
只是选 JSON、YAML 还是一门脚本语言，而是：

> 怎样让数据的表达、验证、转换和最终用途处于同一个可检查、可诊断的模型中？

Forma 是对这个问题的一次语言实验。它提供一个封闭、纯粹、确定且有资源边界
的数据计算世界，再通过少量显式的 Host 入口把结果投射到真实应用。Forma
程序可以构造进程计划、build rule、部署对象或 Agent plan，但它本身不下载
文件、不启动进程，也不获得环境和网络的隐式权限。

这篇介绍先从一个真实需求出发，再解释 Forma 为什么选择这样的边界。

## 一个比 dotslash 更完整的 GCC wrapper

设想一个可以直接运行的 GCC wrapper。它不只是下载单个二进制：

- GCC 与 sysroot 是两个独立包，前者由 Host 平台选择，后者由编译 TARGET 选择；
- `gcc`、`g++` 和 `ar` 共享工具链定义与安装缓存；
- wrapper 要拒绝调用者提供的冲突参数，并自动加入 `--sysroot`、
  `-ffile-prefix-map` 和 `-fdebug-prefix-map`；
- 下载文件和安装目录必须由完整策略确定计算，而不是由外部执行器猜测；
- 错误应指向有问题的 JSON 数据或 Forma 规则；
- 在下载或启动进程之前，应当能看到完整的执行计划。

Forma 仓库中的端到端示例，其 `gcc` 入口就是一个很薄的装配模块：

```forma
#!/usr/bin/env -S forma exec --dry-run --

option "crate.dependency" {
    name: "gcc-toolchain-define",
    source: 'Path({path: "../gcc-toolchain-define"}),
};
option "crate.dependency" {
    name: "gcc-wrapper",
    source: 'Path({path: "../gcc-wrapper"}),
};
option "exec.capture-envs" ["TARGET"];

import "std/rt-types/exec.forma" { ExecFn };
import "gcc-toolchain-define/source.json" as source;
import "gcc-wrapper/toolchain.forma" { command };

export def exec: ExecFn = command("gcc", source);
```

这里没有 GCC 专用语法：依赖是静态 option，工具链描述是 JSON 模块，wrapper
是普通 Forma 模块，`ExecFn` 是 Host 公布的普通类型。`exec.capture-envs` 也
不是“继承整个环境”；它只允许 exec entry 捕获 `TARGET` 并把值作为显式请求
数据交给 main。

共享模块先把外部数据验证为领域类型：

```forma
@struct type Package = {
    name: String,
    src: String,
    digest: String,
};

@struct type ToolchainSource = {
    compilers: Dict(Package),
    sysroots: Dict(Package),
};

def validated_source: Fn(Any) -> ToolchainSource = fn(raw) {
    match validate(ToolchainSource, raw) {
        'Ok(source) => source,
        'Err(error) => reraise!(error),
    }
};
```

随后，普通函数选择包、计算 hash 地址并改写参数：

```forma
def install_dest = fn(settings, package, ty, strip) {
    let identity = `unpack-v1\n\{package.name}\n\{package.src}\n\{package.digest}\n\{ty}\n\{strip}`;
    `\{settings.install_prefix}/\{hash.sha256(identity)}`
};

def checked_compiler_args = fn(request, sysroot_dest) {
    let arguments = match argv.reject_option(request.args, "--sysroot") {
        'Ok(arguments) => arguments,
        'Err(error) => reraise!(error),
    };
    let arguments = match argv.reject_option(arguments, "-ffile-prefix-map") {
        'Ok(arguments) => arguments,
        'Err(error) => reraise!(error),
    };
    let arguments = match argv.reject_option(arguments, "-fdebug-prefix-map") {
        'Ok(arguments) => arguments,
        'Err(error) => reraise!(error),
    };
    argv.prepend([
        `--sysroot=\{sysroot_dest}`,
        `-ffile-prefix-map=\{request.cwd}=.`,
        `-fdebug-prefix-map=\{request.cwd}=.`,
    ], arguments)
};
```

最终值是完全具体、可以编码成 JSON 的 `ExecEnv`：它列出多个 `Unpack` 动作、
每个下载文件与安装目标、工作目录、程序、参数，以及 `{clear, update}` 环境
策略。Host 不需要再替换变量、计算路径或理解 GCC 政策。

今天可以运行：

```sh
TARGET=aarch64-linux-gnu \
  cargo run -p forma -- exec --dry-run \
  examples/gcc-wrapper/app/bin-src/gcc.forma -- \
  -c /workspace/hello.c -o /workspace/hello.o
```

当前 adapter 只输出计划，不下载或执行它。这不是示例缺失的最后一行代码，
而是一条刻意的权限边界：确定计算已经完成，真实效果仍等待 Host 授权。

## 为什么现有方案没有完全覆盖这个位置

Forma 并不声称其他方案“不能编程”。差异在于哪一部分被当作核心，以及当程序
规模增长时，需要由多少套模型共同解释结果。

### 数据格式、schema 与数据工具

JSON 的价值是清楚、稳定、普遍可消费的数据边界。YAML、TOML 和 KDL 针对
人工书写或结构表达做了不同取舍。它们非常适合作为输入和最终产物，但不负责
定义复用、条件选择和转换。

JSON Schema 可以为数据增加共享契约，却不负责计算怎样产生数据。`JSON +
jq/jaq` 则获得了强大的查询、过滤和组合能力；对于一次性转换，这通常是最
直接的工具。应用继续增长后，领域类型可能在 schema 中，转换在 filter 中，
依赖和执行环境在脚本中，最终协议又在 Host 代码中。错误也容易只指向当前
JSON 或 filter，难以统一解释原始数据、拒绝规则、生成步骤和 Host 契约。

Forma 保留“直接转换数据”的体验，但让静态数据、类型、函数、模块、来源和
最终协议进入同一个语义模型。JSON、TOML 和 YAML 在 Forma 中也是模块，不是
失去位置的不透明 blob。

### 通用语言

Python 和 JavaScript 提供开放的动态计算和成熟生态，几乎可以解决任何工程
问题。Python annotations 与 TypeScript 能提前发现大量接口错误，同时有意保留
动态边界。用它们写配置框架时，框架仍需重新定义允许观察什么、如何固定依赖、
怎样限制资源、如何缓存，以及 dry-run 究竟意味着什么。

证明导向语言可以提供远强于 Forma 的性质，但代价是把配置问题带进终止性和
证明工程。Forma 不追求通用定理证明；它允许递归，以确定的 fuel、栈、调用
深度和分配配额给 Host 一个有限执行边界。

Scheme 提供了另一个重要参照：“代码也是数据”带来极大的语言塑造能力，也让
静态分析必须理解展开或运行后产生的代码。Forma 继承的是更窄的想法：

> 类型也是数据，但代码不是任意可生成、可执行的数据。

这种选择保留了一部分元编程价值，同时让模块依赖、名称绑定和可执行函数集合
在求值前保持确定。

### 可编程配置 DSL

CUE、KCL 和 Nickel 最接近 Forma 的问题域。它们已经证明约束、合并、契约和
可编程配置值得拥有专门模型。Forma 的区别不是多几个语法特性，而是尽量把
领域政策还原成普通数据与普通函数。

约束合一、契约应用或字段合并是有价值的 DSL 核心语义；与此同时，每一类
专用规则都需要用户和工具理解它自己的组合及错误传播。Forma 尝试用“类型是
元数据 + 函数 + 少量受控桥梁”覆盖 parse、codec、schema、display、Eq/Hash
等能力，使它们不必各自成为新的语言机制。

这是不同的复杂度配置，不是全面替代宣言。Forma 的主张必须由 GCC wrapper
这样的跨模块、跨数据源案例来检验，而不是由孤立的语法片段证明。

## Forma 的核心模型

### 封闭、纯粹且有界

一个 main 程序能使用的代码、静态数据和依赖图在求值前确定。值不可变，函数
没有外部效果，没有运行时 `eval` 或任意动态 import。递归是允许的，但 Host
为每次执行设置 fuel、栈、调用深度和分配配额。

“封闭”不表示所有输入都是编译期常量。命令行、选定的环境变量和平台信息仍可
进入程序，但必须通过 Host 选择的 entry 显式物化。这样，影响一次计算的世界
既能表达真实请求，又可以枚举、缓存和审计。

### 类型是普通、可编程的元数据

Forma 类型声明产生规范化的不可变数据。类型构造器可以是普通纯函数：

```forma
def Maybe: for(A) Fn(TypeOf(A)) -> TypeOf(Option(A)) = fn(Item) {
    Option(Item)
};

type MaybeInt = Maybe(Int);
```

`TypeOf(A)` 把一个元数据见证与它描述的 `A` 联系起来。同一份定义因而可以
贯穿静态检查、运行时验证、codec、schema、字符串 parse/display 和文档工具，
而不是在每一层复制一份 schema。

普通 Forma 代码还可以解释擦除后的 `TypeDesc`。当一个解释器需要重新获得
静态类型化调用面时，`interpreter!` 提供受限桥梁：

```forma
def my_show: for(A) Fn(TypeOf(A)) -> Fn(A) -> Result(String, BlameError)
    = interpreter!(show_dyn);
```

它不是宏系统或 `eval`：解释算法仍是普通函数，系统只验证擦除参数与声明签名
之间的固定协议。Forma 因而不需要允许用户生成任意代码，仍可以让用户实现
通用的类型导向能力。

### 来源和 blame 穿过数据流

读取 JSON、TOML 或 YAML 时，Forma 不只保留最终值，还保留字段级来源。位置
随值经过 import、验证和转换；规则本身也有来源。错误因此可以同时报告：

```text
source.json:12:9: expected String
  toolchain.forma:16:28: requirement declared here
```

`reraise!` 不会把这种错误压成新字符串。GCC fixture 还验证了错误工具链数据
同时保留 JSON 与 wrapper 规则位置，缺失 `TARGET` 和冲突参数也不会产生部分
dry-run 输出。

这套模型同样服务 LSP。未完成或损坏的源码不会迫使整个工作区退化成“没有
信息”；语义事实区分已知、显式 `Any`、未知、冲突和依赖阻塞，避免用猜测填补
空白。

### 开放世界与封闭世界之间只有轻量连接点

Forma 不需要让 main 获得 IO，也不需要为此引入通用 effect system。Host 先
选择一个受信任的 entry；entry 可以读取受控运行时信息、检查 options，并在
必要时注入带类型见证的虚拟模块。随后它显式初始化 main：

```text
开放的 Host 世界
  -> pending ModuleHandle
  -> 受信任 entry 准备输入与模块
  -> initialize_module（冻结边界）
  -> 封闭的 main 计算
  -> 类型检查后的导出值
  -> dry-run / Host 授权 / 效果
```

main 不能直接 import entry runtime。注入模块属于单次调用，初始化后不可修改，
也不会泄漏到另一个 handle。entry 从 main 取得导出时先得到保留类型方案与来源
的 `Dyn`，再用 `TypeOf(A)` 做权威投影。错误的 `exec` 签名会在调用前被拒绝，
并同时指出 main 定义与 entry 的协议检查点。

这使 `forma exec` 的特殊性主要存在于可替换的 Forma entry 中，而不是散落在
CLI 和 VM 里的 GCC/进程 schema。未来真正的 installer 和 process API 可以只
开放给 entry；main 仍然是普通的纯模块。

## 从一个 wrapper 到更多应用

GCC wrapper 的数据链可以概括为：

```text
锁定依赖与 Host 请求
  -> 外部数据解码和领域验证
  -> 确定转换与参数改写
  -> 完整、类型化的计划
  -> Host 授权和解释
```

更换输入和输出协议，同一模型可以投射到其他领域：

- **build rule**：从源码描述和平台输入生成 artifact DAG 或 `OutputPlan`；
- **增强型 dotslash**：组合多个包、共享安装、改写参数，并在启动前 dry-run；
- **Helm chart / 部署计划**：把多来源数据验证为可审查的 Kubernetes 对象；
- **Agentic plan IR**：把模型生成的意图收敛成 Host 可以比较、签名、批准或
  拒绝的确定计划；
- **数据迁移与生成**：读取 JSON/TOML/YAML，经类型化转换后输出结构化数据或
  稳定文本。

Forma 尚未内置这些领域的完整框架，也没有生产级包获取、真实 exec/install
效果或长期兼容性承诺。它已经验证的是共同的纵向基础：数据输入、可编程类型
元数据、普通函数抽象、来源诊断、有界求值、模块复用，以及受控 Host entry。

## Forma 想证明什么

Forma 的目标不是成为更小的 Python，也不是以另一套专用约束语义取代所有配置
DSL。它想验证一个更具体的判断：

> 以“类型也是数据”为中心，可以用更少、更统一、也更容易解释的机制表达数据、
> 约束和转换，同时保留足够的抽象能力、可复用性、诊断质量与真实应用空间。

如果每增加一个应用领域都需要新的 VM 指令、语言级 effect 或编译器特例，这个
实验就失败了。如果新的领域主要增加普通 Forma 库、类型化协议和狭窄 Host
adapter，而核心语义仍保持小而一致，那么这条路线就值得继续。

## 继续阅读和运行

- [README.zh.md](README.zh.md) 是能力导览和快速使用入口；
- [VISION.md](VISION.md) 给出设计命题与功能准入原则；
- [rfc/](rfc/) 保存逐步实现与验收证据；
- [examples/gcc-wrapper/](examples/gcc-wrapper/) 是本文使用的可执行案例。

```sh
cargo test --workspace

TARGET=aarch64-linux-gnu \
  cargo run -p forma -- exec --dry-run \
  examples/gcc-wrapper/app/bin-src/gcc.forma -- \
  -c hello.c -o hello.o
```
