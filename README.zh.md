# Forma

> Forma 原名 XL，发布后更名。设计演进的全过程记录在 [rfc/](rfc/)。

**Forma 是一门实验性语言，它想回答一个问题：当类型只是普通数据的时候，一门语言会变成什么样。**

配置语言已经探索出几条成熟路线：CUE 以合一（unification）组织约束，Dhall 以强规范化换取可判定的执行边界，Starlark 提供受控的通用计算，Nickel 则把契约与配置合并作为核心抽象。这些选择各自解决了真实问题，也把相应的领域概念放进了语言机制。

Forma 赌的是另一个方向：**领域语义应该住在数据里，语言只提供计算**。配置、校验、规范化、编解码、schema 生成——这些不是语言特性，而是普通函数对普通数据的普通操作。

## 三个相互咬合的赌注

### 一种语言，两个阶段

Forma 有工具阶段和程序阶段，但它们共享同一套值模型、同一个字节码 VM、同一份求值语义。没有独立的"类型层语言"，没有宏语言，没有编译期的第二套求值器。工具阶段跑的就是普通 Forma 代码——有燃料上限、有配额、确定性、可缓存。

### 类型即元数据

类型声明求值的结果不是只能由编译器访问的内部结构，而是普通的 Forma 数据：

```forma
def Maybe: for(A) Fn(TypeOf(A)) -> TypeOf(Option(A)) = fn(Item) {
    Option(Item)
};

type MaybeInt = Maybe(Int);
```

`Maybe` 不是类型操作符，它是一个普通函数，在工具阶段被普通地调用，返回普通的数据。类型检查器不重新实现它的函数体——它解释这份数据。这意味着：

- **类型可以被打印**：`debug.dbg(User)` 把你的类型原样打出来，因为它就是值
- **类型可以被函数变换**：`Partial(User)`、`Array(User)` 都是函数调用
- **类型知道自己描述谁**：`TypeOf(A)` 让 `decode(User, input)` 的类型是 `Result(User, BlameError)` 而不是 `Result(Any, Any)`——实例类型穿越边界，全程不丢

### 封闭世界

模块路径静态可知，依赖固定，无运行时 `eval`，外部数据经显式边界进入。封闭世界不是限制，是杠杆：它让编译期求值可再生、让工具链能枚举全部代码、让"在别人的进程里安全地跑别人的配置"成为默认能力——每次执行有燃料、栈、分配、深度的独立配额，失败原子地丢弃，不污染共享世界。

## 这些理念长出了什么

**Serde，但没有宏。** 装饰器是普通函数，属性是普通数据，codec 是元数据的普通解释器：

```forma
import json from "@bim/std/json";

@json.rename_all('CamelCase)
@struct
type User = {
    user_id: Int,
    @json.default('None)
    nickname: Option(String),
};
```

字段重命名、默认值、扁平化、跳过条件——全部是库函数写在元数据上的标注，编解码器按同一份计划双向工作，JSON Schema 也从同一份计划生成。语言本身对这些词汇一无所知。

**会指路的错误信息。** 每个值都带着自己的源码位置旅行——穿过导入、变换、codec 归一化。校验失败时：

```text
user.json:1:21: expected Int
  User.forma:3:47: contract rule declared here
```

错误同时标出数据位置和规则位置。Forma 的 `BlameError` 直接保留这两类来源，不需要为 codec 另建一套与值模型分离的诊断结构。

**可执行的计划。** Forma 可以表达一种能力更丰富的 DotSlash：入口模块是普通的纯函数 `Fn(ExecSettings, ExecRequest) -> Exec`，宿主显式传入平台、缓存前缀、环境、参数和工作目录，函数返回已经完全确定的执行计划。它可以选择多个工件，例如把平台相关的解释器与平台无关的运行时分别安装；可以用 `hash.sha256` 计算稳定的下载和安装位置；也可以构造搜索路径、库路径和环境变量。

命令行改写同样属于这段纯计算。一个 gcc 或 rustc 启动器可以根据计算出的安装位置补充 sysroot 和平台相关搜索路径，注入 `source-prefix-map`，再改写用户传入的源文件参数。返回的 `Exec` 已经包含最终的 command、args、env 和路径：宿主不展开模板、不替换变量，也不重新解释策略。这里没有特殊的上下文模块或启动器语法，只有显式参数、普通函数和可 JSON 化的数据；内外世界的连接点因此很薄。

例如，一个可再生的 gcc 启动计划可以完整地写成：

```forma
#!/usr/bin/env -S forma exec --dry-run

import arrays from "@bim/std/array";
import hash from "@bim/std/hash";

fn(settings, request) {
    let platform = "\{settings.platform.os}-\{settings.platform.arch}";
    let toolchain_url = "https://example.invalid/gcc-\{platform}.tar.zst";
    let sysroot_url = "https://example.invalid/sysroot-\{platform}.tar.zst";
    let toolchain_cache = "\{settings.cache_prefix}/\{hash.sha256(toolchain_url)}";
    let sysroot_cache = "\{settings.cache_prefix}/\{hash.sha256(sysroot_url)}";
    let toolchain = "\{settings.install_prefix}/\{hash.sha256("gcc:\{toolchain_url}:unpack-v1")}";
    let sysroot = "\{settings.install_prefix}/\{hash.sha256("sysroot:\{sysroot_url}:unpack-v1")}";
    let args: Array(String) = arrays.flat_map([
        [
            "--sysroot=\{sysroot}",
            "-isystem\{sysroot}/usr/include",
            "-ffile-prefix-map=\{request.cwd}=.",
        ],
        request.args,
    ], fn(part) { part });

    {
        downloads: [
            {url: toolchain_url, cache: toolchain_cache},
            {url: sysroot_url, cache: sysroot_cache},
        ],
        installs: [
            {name: "gcc", source: toolchain_cache, path: toolchain},
            {name: "sysroot", source: sysroot_cache, path: sysroot},
        ],
        command: "\{toolchain}/bin/gcc",
        args: args,
        env: {FORMA_SYSROOT: sysroot},
        cwd: request.cwd,
    }
}
```

当前的 `forma exec --dry-run` 只校验并输出这份计划，不下载、不安装，也不启动进程。即便效果层尚未实现，所有确定性决策已经可以被审查、纳入版本控制并独立测试。未来的宿主只需消费这份具体计划并执行效果。同一边界也适用于构建规则和 K8s 调谐：**纯函数生成计划，宿主执行效果**。

**一个保守的语言服务器。** 悬停看到的类型来自与运行时校验相同的元数据；遇到 `Any` 时，补全不会推测不存在的结构；残缺的源码仍然可以产生导航和诊断，并明确区分"未知、冲突、不可计算"等状态。

## 设计取舍

- **与 CUE 相比**：Forma 不以合一作为约束与组合的基础语义。类型是数据，校验和组合策略是显式函数。
- **与 Dhall 相比**：两者都重视纯计算和可再生结果；Dhall 通过强规范化保证终止，Forma 允许递归并用执行燃料和资源配额建立边界。
- **与 Starlark 相比**：两者都适合在宿主中执行受控代码；Forma 进一步让类型元数据参与普通计算，并让静态工具和运行时解释同一份元数据。
- **与 Nickel 相比**：Nickel 把契约、合并和优先级作为配置领域的核心机制；Forma 倾向于把这些策略表达成可以检查、替换和组合的库函数。

Forma 的取舍不是消除复杂度，而是尽量把领域复杂度放进库和数据。库可以被阅读、替换和扩展，语言核心则保持较小且一致。

## 诚实的边界

Forma 是实验品。它今天没有效果系统、没有包获取（只有路径依赖）、没有 YAML/TOML 解析器、没有 trait、没有类型收窄。静态推断宁可显式说"不知道"，也不猜测。这些不是疏忽，是刻意：先把"类型即元数据"这一个假设验证到根，再谈扩张。60 份 RFC 记录了每一步的取舍——包括每个被拒绝的替代方案。

Forma 面向的场景也由此变得清晰：**构建规则的表达、K8s operator 的持续调谐、可复用的配置包**——宿主需要确定地执行外部提供的逻辑，并在失败时解释数据与规则来自何处。

## 试一试

```sh
cargo run -p forma -- check examples/mvp/main.forma
cargo run -p forma -- run examples/mvp/external.forma --input examples/mvp/request.json
cargo run -p forma -- show examples/mvp/main.forma
cargo run -p forma-lsp -- --help
```

## 文档

- [VISION.md](VISION.md)：设计命题
- [rfc/](rfc/)：60 份设计文档，每份含被拒方案与验收标准
- [README.md](README.md)：English

## 验证

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
