# Forma

> **Forma 是 XL 语言的新名字。** 项目推送至远端仓库后完成改名；改名决策与范围见
> [rfc/0060](rfc/0060-rename-xl-to-forma.md)。历史 RFC 文档保留当初的 XL 措辞，
> 作为设计过程的记录。

Forma 是一门实验性通用编程语言，拥有不可变的动态字节码运行时，以及封闭世界、
两阶段的类型元数据模型。设计命题见 [VISION.md](VISION.md)，设计演进序列见
[rfc/](rfc/)。English documentation: [README.md](README.md)。

核心假设：在足够封闭的世界里，类型就是普通的不可变元数据，而高阶类型操作就是
普通的纯函数——由工具链内嵌的同一个语言运行时求值。

语言当前已验证的能力：

- 类 Rust 的表达式语法，`fn(args) { ... }` 闭包与 `|>` 管道；
- 不可变的 Dict、Array、Tuple、Atom，以及一等的 `'Tag(payload)` Tagged 值；
- 带 payload 类型传播的模式匹配；
- 显式单次赋值递归（`decl`/`def`）与 proper tail call；
- 普通函数在工具阶段 VM 中计算类型元数据；
- 同一份元数据同时用于结构注解检查、运行时校验与派生 JSON codec；
- `@struct`/`@enum`/`@union` 归一化模型与扁平值属性；
- `native`、`decl`、`def` 绑定上的显式前束泛型契约（`for(A, B)`），运行时完全擦除；
- `Type` 与 `TypeOf(A)` 元数据见证，`decode(User, input)` 的类型是
  `Result(User, BlameError)`；
- crate 相对的模块身份（`@src/...`、依赖别名、`@bim/std/...`）与封闭依赖图；
- 语言服务器：诊断、悬停、定义跳转、引用查找与保守补全，构建在可恢复的语义快照之上；
- 通过 `forma exec --dry-run` 输出可审查的执行计划。

## 试一试

```sh
cargo run -p forma -- check examples/mvp/main.forma
cargo run -p forma -- run examples/mvp/external.forma --input examples/mvp/request.json
cargo run -p forma -- show examples/mvp/main.forma
cargo run -p forma-lsp -- --help
```

## 语法速览

```xl
@struct
type User = {
    name: String,
    age: Int,
    nickname: Option(String),
};

let user: User = imported_user;
validate(User, user)
```

`value |> f` 与 `f(value)` 完全等价。显式调用节（call section）用 `\(` 与占位符
构造普通闭包：

```text
transform\(_, option)
// 等价于 fn(value) { transform(value, option) }

reorder\(_1, fixed, _0)
// 等价于 fn(a, b) { reorder(b, fixed, a) }
```

裸占位符按出现顺序生成参数；带下标的占位符可以重排或复用参数，且必须构成从
`_0` 开始的连续区间。

集合操作就是普通的导入函数：

```xl
import arrays from "@bim/std/array";
import dicts from "@bim/std/dict";

[1, 2, 3]
    |> arrays.map\(_, fn(value) { value + 1 })
    |> arrays.filter\(_, fn(value) { 2 < value })
```

派生 codec 让外部数据边界显式且有类型：

```xl
import data from "./abc.json";
import User from "./User.xl";
import result from "@bim/std/result";
import json from "@bim/std/json";

let user = data |> User.decode |> result.unwrap;
// user : User
user |> User.encode |> json.stringify_pretty(2)
```

见 `examples/codec`。枚举值是一等的：`'None` 是 Atom，`'Some("Ada")` 是 Tagged
值。每个 Atom 都是一元构造器，所以 `arrays.map([1, 2], 'Some)` 可以直接写。

## 类型元数据

类型声明求值为规范的 Dict/Array/Atom 元数据——普通 Forma 表达式也能构造同样的
数据。泛型元数据函数就是普通的带注解定义：

```xl
def Maybe: Fn(Type) -> Type = fn(Item) {
    Option(Item)
};

type MaybeInt = Maybe(Int);
```

内建的校验与 codec 契约携带见证（witness），实例类型穿越边界而不丢失：

```xl
native decode: for(A) Fn(TypeOf(A), Any) -> Result(A, BlameError);
native unwrap: for(A, E) Fn(Result(A, E)) -> A;
```

## 执行世界

运行时所有权分固定的两层。内建模块与已初始化的模块导出住在 `MainWorld`；每次
模块初始化或服务调用在全新的 `WorkWorld` 中分配。VM 执行只读 Main、只写 Work。
模块发布把可达的 Work 值原子地复制进 Main。加载完成后 Main 被冻结；服务结果
直接导出，Work 整体丢弃——重复的会话无法改写已加载的应用。

## 工具链

`forma-lsp` 二进制提供异步语言服务器：带跨文件 blame 标签的诊断、悬停查看计算
出的类型元数据、定义与引用导航、针对模块导出和 Struct 字段的保守补全。
`forma show`、测试与 LSP 适配器共享同一份不可变工作区快照；不完整的源码也能
恢复出语法、定义与显式的事实状态，而不是猜测出的类型。

## 可执行计划

Forma 模块可以求值为一个纯的计划函数。宿主提供不可变的调用输入，模块计算出
具体结果：

```xl
#!/usr/bin/env -S forma exec --dry-run

fn(settings, request) {
    {
        install: [],
        command: "python3",
        args: request.args,
        env: request.env,
        cwd: request.cwd,
    }
}
```

`forma exec --dry-run tool.xl -- arg1 arg2` 打印规范的 JSON 计划。所有确定性
决策都在 Forma 内；当前阶段宿主不做下载、安装，也不创建进程。

## 当前限制

Forma 没有效果系统、路径依赖之外的包获取、YAML/TOML 解析器、trait、类型收窄
（narrowing），也没有生产级垃圾回收。静态推断宁可显式不可用或降级为 `Any`，
也不猜测。各 RFC 的 deferred work 一节有完整的诚实清单。

## 验证

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```
