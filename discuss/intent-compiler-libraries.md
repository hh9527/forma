# Forma 领域意图编译器

- Stage: Discussion
- Scope: high-level plans, domain libraries, verified lowering, diagnostics
- Primary thought experiments: `gcc-wrapper.md`, `intelligent-reporting-intent-compiler.md`

## 问题

Forma 能否同时作为高阶意图的可编程表达载体，以及构造领域意图编译器的
语言？

目标不是让 Forma 程序直接获得现实世界权限。Code Agent 编写 Forma，表达
靠近业务意图的高阶计划；预先发布的 Forma 领域库承载领域知识，在校验这份
计划的同时完成 lowering；成功结果成为 Host 可授权执行的低阶计划，失败结果
成为 Code Agent 可以直接修复的编译诊断。

```text
Code Agent 编写 Forma
        |
        v
高阶计划 / 领域意图
        |
        v
版本化 Forma 领域库
    verification + lowering
        |
        +-- diagnostics -> Code Agent 修复
        |
        v
低阶执行计划
        |
        v
Host 授权并干预真实世界
```

领域库可能由另一个 Code Agent、人类或双方协作在两天前发布。重要的不是谁
编写它，而是领域知识已经成为可版本化、可 review、可测试、可复用的普通软件
资产，不必重新塞进当前模型的 prompt。

## 三个观察角度

“可编程配置”、“意图编译器”和“领域规则建模语言”描述的是同一个模型：

```text
可编程配置
    从具体意图如何表达来看

意图编译器
    从具体意图如何被处理来看

领域规则建模语言
    从领域库如何定义处理语义来看
```

Forma 通过类型、普通数据和纯 transform 定义领域概念、normalization、
verification、lowering、linking 与 plan construction。具体程序使用这些能力
表达意图。Host 只提供显式事实并解释最终计划，不维护另一份业务规则。

## 领域库是领域知识的载体

领域库不只是一组事后 predicate。它可以定义：

- 领域中存在哪些概念；
- 高阶意图可以怎样组合；
- 表面差异如何规范化；
- 抽象意图如何逐步 lowering；
- 每个阶段必须满足哪些局部和全局约束；
- 哪些 Host context 会影响 lowering；
- 最终计划必须满足什么协议；
- 失败时怎样解释意图与规则的冲突。

理想的编译路径由普通函数组成：

```forma
export def compile:
    Fn(Context, Intent) -> Compilation(Plan) =
    fn(context, intent) {
        let normalized = normalize(intent);
        let resolved = resolve(context, normalized);
        let lowered = lower(context, resolved);
        link_and_validate(context, lowered)
    };
```

这里的 `Compilation` 是概念形状，不是已确定的标准库 API。

## 校验与 lowering 是同一个过程

意图通常不能先被完整校验，再机械转换。很多合法性只有 lowering 时才知道：

- 是否存在合法且唯一的关系路径；
- 抽象能力能否在目标平台实现；
- lowering 是否改变 grain 或引入重复计算；
- 权限是否允许所需的具体资源；
- 目标方言是否支持所需表示；
- 最终计划的多个输出是否保持一致。

因此每个阶段更接近：

```text
A -> Compilation(B)
```

它在验证 `A` 的同时产生更具体、约束更强的 `B`。只有满足当前阶段规则的
意图才能进入下一阶段。纯检查只是退化形式：

```text
A -> Compilation(A)
```

## 类型与数据不是竞争路线

稳定、局部并能排除一整类无意义组合的关系，应优先使用 Forma 类型表达：

```forma
eq: for(A) Fn(Expr(A), Expr(A)) -> Predicate;
```

异质 registry、动态引用、路径搜索、权限、版本和 Host context 更适合用普通
数据与 transform 表达。类型元数据可以连接两边：静态 facade 提供早期检查，
data-level descriptor 支持开放本体和领域化反馈。

选择类型的条件：

- 关系在局部调用点已经确定；
- 对大多数调用稳定成立；
- 能用现有 rank-1 泛型和结构自然表达；
- 不要求类型检查器执行领域图搜索或上下文推理。

选择数据的条件：

- 知识来自 registry、配置或 Host context；
- 需要遍历、搜索、候选选择或丰富修复建议；
- 会随租户、权限、版本或目标环境改变；
- 强行类型化会引入依赖类型、trait/assoc type 或复杂名义体系。

理想路线不是把所有领域知识推入 Forma 内核，而是让小而可靠的静态基础支撑
可执行的领域模型。

## 编译结果不能只有 fail-fast Result

Code Agent 的收敛速度取决于一轮反馈的信息量。一次只暴露一个错误会导致：

```text
发现 A -> 修复 A -> 发现 B -> 修复 B -> 发现 C
```

更好的结果是：

```text
发现 A、B、C，并说明 D 被 A 阻塞
    -> 一次修复多个独立根因
```

但目标不是 error flood，而是三个同时成立的指标：

```text
Coverage
    一轮发现尽可能多的独立问题

Precision
    每个问题指向真实意图和真实规则

Causality
    区分根因、级联结果和 blocked 检查
```

“识别全部错误”的可检验定义应当是：

> 在本轮资源边界内，发布所有能够由当前可靠事实独立确定的问题；依赖失败
> 根因的检查标记为 blocked，而不制造级联误报。

## 编译结果分类

概念上需要区分三种结果：

```text
Complete {
    plan: Plan,
    diagnostics: [],
}

Rejected {
    diagnostics: NonEmptyArray(Diagnostic),
    blocked: Array(BlockedCheck),
}

Aborted {
    resource_failure: Fuel | Allocation | Cancelled | ...,
}
```

- `Complete` 表示所有必要 lowering 阶段完成；
- `Rejected` 表示领域意图不成立；
- `Aborted` 表示本次编译未完成，不能误报为业务拒绝或成功。

不存在 `diagnostics = []` 且 `plan = None` 的普通完成状态。存在权威领域诊断
时也不能发布一个仿佛完整的 Plan。

## 根因诊断

诊断应尽量在领域词汇仍然完整的最高层报告问题，而不是只指出低阶 IR 或 Host
错误。一个高价值诊断至少连接：

```text
具体意图位置
    +
直接拒绝它的领域规则位置
```

结构化诊断还可以包含：

- 稳定 category/ID；
- lowering stage；
- 实际与期望的领域形状；
- cause 和 blocked checks；
- 可用候选或 repair suggestions；
- 依赖的外部数据来源。

provenance、blame、recoverable analysis 与 best-effort facts 在这里不是编辑器
附属能力，而是 Code Agent 修复 loop 的接口。

## 最大独立诊断覆盖需要什么

线性的 `Result + ?` 会在第一个错误停止。领域 compiler 更接近一张依赖图：

```text
resolve A ───> lower A ──┐
                         ├─> link -> Plan
resolve B ───> lower B ──┘

resolve C ───> lower render
```

A 失败后，lower A 与 link 可以标记为 blocked，但 B、C 仍应继续。实现路线可以
组合：

- best-effort `Never`/blocked propagation；
- 显式的 validation combinator；
- 受控、write-only 的 diagnostic accumulation；
- query dependency graph 与稳定 facts。

这重新提出 `typed-accumulation-channels.md` 中的问题，但不自动接受通用 effect
system。需要的能力很窄：一次意图编译期间收集辅助诊断，失败分支阻塞其依赖，
独立分支继续，结束后一次性读取完整集合。

## Plan 的保证边界

一旦得到 Plan，Forma 应保证：

- 结构符合 Host protocol；
- 引用全部 resolve；
- 领域规则和 lowering 前提已经满足；
- 没有留给 Host 猜测的变量替换或业务决策；
- 多个输出（例如 SQL、result schema、render template）保持一致；
- 在相同库版本和显式 Context 下结果确定。

Forma 不能保证网络、数据库或外部服务在执行时不失败。更准确的承诺是：Host
执行时只需处理真实世界失败，不应再发现本可由领域 compiler 识别的结构或业务
错误。会变化的外部状态可以用 context revision 和显式 assumptions 固定，执行
前失效则重新编译。

## 领域规则的单一来源

理想边界是：

```text
Forma 领域库
    领域概念、ontology、verification、lowering、linking、plan validation

Host
    提供外部事实、授权、执行效果
```

如果规则依赖 catalog、权限或平台，Host 将这些事实作为显式 Context 提供；
规则判断仍由 Forma 库完成。Host 不应再维护一份平行的业务 checker。

## 简单性是功能要求

Forma 面向“表达意图、编译意图、依据诊断修复意图”的 loop，因此表达能力和
诊断能力同等重要。语法和内核概念的简单化不是语言审美，而是为了：

- 降低 Code Agent 生成与修改意图的错误率；
- 让领域规则代码接近业务推理；
- 让诊断忠实映射到实际源码，而非隐藏展开；
- 让规则易于阅读、测试和排障；
- 让新领域主要增加库，而不是 VM 指令与编译器特例。

理想概念预算保持在：不可变数据、类型与 `TypeOf(A)`、普通函数、模式匹配、
模块、结构化诊断、provenance/blame、受控 accumulation 和 Host entry。若领域
库反复需要 trait、assoc type、依赖类型、通用 effect 或宏系统，应先重新检查
问题能否由普通数据和 transform 忠实表达。

## 验收方向

一个领域意图 compiler 应追求：

1. 可判定的领域越界不能产生 Plan；
2. 一轮发布所有独立可确定的问题；
3. 诊断靠近高阶意图及直接规则，并抑制级联噪音；
4. 无领域诊断且无资源中止时必定产生完整 Plan；
5. Plan 不含未 resolve 引用或隐式领域决策；
6. lowering 的语义保持由测试和领域不变量验证；
7. verification/lowering 规则只在 Forma 库中实现一次；
8. 规则代码主要由类型、数据和普通 transform 构成；
9. 领域拒绝与资源中止严格区分；
10. feedback 足以让 Code Agent 一轮修复多个独立根因。

## 非目标

- 让 Forma 程序直接拥有现实世界效果；
- 为每个领域向 VM 加入专用概念；
- 声称发现任意递归程序中数学意义上的全部错误；
- 保证外部世界永不发生运行时失败；
- 把 AI、常驻服务或某一种 Agent 协议写入语言语义。

常驻服务、LSP 和增量缓存可以改善工程体验，但最小模型只是 Code Agent 编写
Forma、运行 compiler、读取 diagnostics、修改代码，直到得到 Host 可执行计划。

