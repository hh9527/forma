use crate::ast::Program;
use crate::hir::HirResolution;
use crate::source::{Location, SourceDatabase, SourceId};
use crate::types::{Analysis, TypeGraph, TypeId, TypeNode};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

macro_rules! compact_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(u32);

        impl $name {
            pub const fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

compact_id!(WorkspaceModuleId);
compact_id!(DefinitionId);
compact_id!(ReferenceId);
compact_id!(WorkspaceExpressionId);
compact_id!(WorkspaceTypeId);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceModuleKind {
    Xl,
    Json,
    Core,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceImport {
    pub name: String,
    pub location: Location,
    pub target: WorkspaceModuleId,
}

#[derive(Clone, Debug)]
pub struct WorkspaceModule {
    pub id: WorkspaceModuleId,
    pub name: String,
    pub path: Option<PathBuf>,
    pub kind: WorkspaceModuleKind,
    pub source: Option<SourceId>,
    pub imports: Vec<WorkspaceImport>,
    pub result_location: Option<Location>,
    pub result_type: Option<WorkspaceTypeId>,
}

pub use crate::hir::HirDefinitionKind as DefinitionKind;

#[derive(Clone, Debug)]
pub struct Definition {
    pub id: DefinitionId,
    pub module: WorkspaceModuleId,
    pub name: String,
    pub kind: DefinitionKind,
    pub location: Location,
    pub additional_locations: Vec<Location>,
    pub ty: Option<WorkspaceTypeId>,
    pub import_target: Option<WorkspaceModuleId>,
}

impl Definition {
    fn contains(&self, location: Location) -> bool {
        contains(self.location, location)
            || self
                .additional_locations
                .iter()
                .any(|candidate| contains(*candidate, location))
    }
}

#[derive(Clone, Debug)]
pub struct Reference {
    pub id: ReferenceId,
    pub module: WorkspaceModuleId,
    pub name: String,
    pub location: Location,
    pub definition: Option<DefinitionId>,
    pub external: bool,
}

#[derive(Clone, Debug)]
pub struct WorkspaceExpression {
    pub id: WorkspaceExpressionId,
    pub module: WorkspaceModuleId,
    pub location: Location,
    pub reference: Option<ReferenceId>,
    pub ty: Option<WorkspaceTypeId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceExport {
    pub name: String,
    pub ty: WorkspaceTypeId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceTypeNode {
    Pending,
    Ref(WorkspaceTypeId),
    Any,
    Int,
    Float,
    String,
    Bytes,
    Atom(String),
    Array(WorkspaceTypeId),
    Tuple(Vec<WorkspaceTypeId>),
    Struct(BTreeMap<String, WorkspaceTypeId>),
    Enum(BTreeMap<String, Option<WorkspaceTypeId>>),
    Union(Vec<WorkspaceTypeId>),
    Function {
        parameters: Vec<WorkspaceTypeId>,
        result: WorkspaceTypeId,
    },
}

#[derive(Clone, Debug, Default)]
pub struct WorkspaceTypeGraph {
    nodes: Vec<WorkspaceTypeNode>,
    names: BTreeMap<String, WorkspaceTypeId>,
}

impl WorkspaceTypeGraph {
    pub fn nodes(&self) -> impl ExactSizeIterator<Item = (WorkspaceTypeId, &WorkspaceTypeNode)> {
        self.nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (WorkspaceTypeId(index as u32), node))
    }

    pub fn node(&self, id: WorkspaceTypeId) -> Option<&WorkspaceTypeNode> {
        self.nodes.get(id.index())
    }

    pub fn names(&self) -> impl Iterator<Item = (&str, WorkspaceTypeId)> {
        self.names.iter().map(|(name, id)| (name.as_str(), *id))
    }

    pub fn display(&self, id: WorkspaceTypeId) -> Option<String> {
        self.node(id)?;
        Some(self.display_with(id, &mut HashSet::new()))
    }

    fn display_with(&self, id: WorkspaceTypeId, active: &mut HashSet<WorkspaceTypeId>) -> String {
        if !active.insert(id) {
            return self
                .names
                .iter()
                .find_map(|(name, candidate)| (*candidate == id).then(|| name.clone()))
                .unwrap_or_else(|| "recursive".into());
        }
        let shown = match &self.nodes[id.index()] {
            WorkspaceTypeNode::Pending => "<pending>".into(),
            WorkspaceTypeNode::Ref(target) => self.display_with(*target, active),
            WorkspaceTypeNode::Any => "Any".into(),
            WorkspaceTypeNode::Int => "Int".into(),
            WorkspaceTypeNode::Float => "Float".into(),
            WorkspaceTypeNode::String => "String".into(),
            WorkspaceTypeNode::Bytes => "Bytes".into(),
            WorkspaceTypeNode::Atom(atom) => format!("'{atom}"),
            WorkspaceTypeNode::Array(item) => {
                format!("Array<{}>", self.display_with(*item, active))
            }
            WorkspaceTypeNode::Tuple(items) => format!(
                "({})",
                items
                    .iter()
                    .map(|item| self.display_with(*item, active))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            WorkspaceTypeNode::Struct(fields) => format!(
                "{{{}}}",
                fields
                    .iter()
                    .map(|(name, item)| format!("{name}: {}", self.display_with(*item, active)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            WorkspaceTypeNode::Enum(variants) => format!(
                "enum {{{}}}",
                variants
                    .iter()
                    .map(|(name, payload)| payload.map_or_else(
                        || name.clone(),
                        |payload| format!("{name}({})", self.display_with(payload, active))
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            WorkspaceTypeNode::Union(items) => items
                .iter()
                .map(|item| self.display_with(*item, active))
                .collect::<Vec<_>>()
                .join(" | "),
            WorkspaceTypeNode::Function { parameters, result } => format!(
                "fn({}) -> {}",
                parameters
                    .iter()
                    .map(|item| self.display_with(*item, active))
                    .collect::<Vec<_>>()
                    .join(", "),
                self.display_with(*result, active)
            ),
        };
        active.remove(&id);
        shown
    }
}

#[derive(Clone, Debug)]
pub struct WorkspaceSnapshot {
    sources: SourceDatabase,
    modules: Vec<WorkspaceModule>,
    definitions: Vec<Definition>,
    references: Vec<Reference>,
    expressions: Vec<WorkspaceExpression>,
    types: WorkspaceTypeGraph,
}

impl WorkspaceSnapshot {
    pub fn sources(&self) -> &SourceDatabase {
        &self.sources
    }

    pub fn modules(&self) -> &[WorkspaceModule] {
        &self.modules
    }

    pub fn module(&self, id: WorkspaceModuleId) -> Option<&WorkspaceModule> {
        self.modules.get(id.index())
    }

    pub fn module_by_path(&self, path: &Path) -> Option<&WorkspaceModule> {
        self.modules
            .iter()
            .find(|module| module.path.as_deref() == Some(path))
    }

    pub fn definitions(&self) -> &[Definition] {
        &self.definitions
    }

    pub fn definition(&self, id: DefinitionId) -> Option<&Definition> {
        self.definitions.get(id.index())
    }

    pub fn definition_at(&self, location: Location) -> Option<&Definition> {
        self.definitions
            .iter()
            .filter(|definition| definition.contains(location))
            .min_by_key(|definition| definition.location.end - definition.location.start)
    }

    pub fn references(&self) -> &[Reference] {
        &self.references
    }

    pub fn reference(&self, id: ReferenceId) -> Option<&Reference> {
        self.references.get(id.index())
    }

    pub fn reference_at(&self, location: Location) -> Option<&Reference> {
        self.references
            .iter()
            .filter(|reference| contains(reference.location, location))
            .min_by_key(|reference| reference.location.end - reference.location.start)
    }

    pub fn references_of(&self, definition: DefinitionId) -> Vec<&Reference> {
        self.references
            .iter()
            .filter(|reference| reference.definition == Some(definition))
            .collect()
    }

    pub fn expressions(&self) -> &[WorkspaceExpression] {
        &self.expressions
    }

    pub fn expression(&self, id: WorkspaceExpressionId) -> Option<&WorkspaceExpression> {
        self.expressions.get(id.index())
    }

    pub fn expression_at(&self, location: Location) -> Option<&WorkspaceExpression> {
        self.expressions
            .iter()
            .filter(|expression| contains(expression.location, location))
            .min_by_key(|expression| expression.location.end - expression.location.start)
    }

    pub fn type_of_expression(&self, id: WorkspaceExpressionId) -> Option<WorkspaceTypeId> {
        self.expression(id).and_then(|expression| expression.ty)
    }

    pub fn type_at(&self, location: Location) -> Option<WorkspaceTypeId> {
        if let Some(reference) = self.reference_at(location)
            && let Some(ty) = reference
                .definition
                .and_then(|id| self.definition(id))
                .and_then(|definition| definition.ty)
        {
            return Some(ty);
        }
        if let Some(definition) = self.definition_at(location)
            && let Some(ty) = definition.ty
        {
            return Some(ty);
        }
        self.expression_at(location)
            .and_then(|expression| expression.ty)
    }

    pub fn types(&self) -> &WorkspaceTypeGraph {
        &self.types
    }

    pub fn exports_of(&self, module: WorkspaceModuleId) -> Vec<WorkspaceExport> {
        let Some(result) = self.module(module).and_then(|module| module.result_type) else {
            return Vec::new();
        };
        let Some(WorkspaceTypeNode::Struct(fields)) = self.types.node(result) else {
            return Vec::new();
        };
        fields
            .iter()
            .map(|(name, ty)| WorkspaceExport {
                name: name.clone(),
                ty: *ty,
            })
            .collect()
    }

    pub(crate) fn build(sources: SourceDatabase, mut inputs: Vec<SemanticModuleInput>) -> Self {
        let mut core_names = inputs
            .iter()
            .flat_map(|input| input.imports.iter())
            .filter_map(|import| match &import.target {
                SemanticModuleTarget::Core(name) => Some(name.clone()),
                SemanticModuleTarget::Path(_) => None,
            })
            .collect::<HashSet<_>>();
        for name in core_names.drain() {
            inputs.push(SemanticModuleInput {
                key: name.clone(),
                path: None,
                kind: WorkspaceModuleKind::Core,
                source: None,
                program: None,
                analysis: None,
                imports: Vec::new(),
            });
        }
        inputs.sort_by(|left, right| left.key.cmp(&right.key));

        let ids = inputs
            .iter()
            .enumerate()
            .map(|(index, input)| (input.key.clone(), WorkspaceModuleId(index as u32)))
            .collect::<HashMap<_, _>>();
        let mut types = WorkspaceTypeGraph::default();
        let mut type_maps = Vec::with_capacity(inputs.len());
        for input in &inputs {
            let map = input.analysis.as_ref().map_or_else(Vec::new, |analysis| {
                merge_type_graph(&input.key, &analysis.types, &mut types)
            });
            type_maps.push(map);
        }

        let mut modules = Vec::with_capacity(inputs.len());
        for (index, input) in inputs.iter().enumerate() {
            let id = WorkspaceModuleId(index as u32);
            let imports = input
                .imports
                .iter()
                .map(|import| WorkspaceImport {
                    name: import.name.clone(),
                    location: import.location,
                    target: ids[&import.target.key()],
                })
                .collect();
            let result_type = input
                .analysis
                .as_ref()
                .map(|analysis| type_maps[index][analysis.result_type.index()]);
            modules.push(WorkspaceModule {
                id,
                name: input.key.clone(),
                path: input.path.clone(),
                kind: input.kind,
                source: input.source,
                imports,
                result_location: input
                    .program
                    .as_ref()
                    .map(|program| program.value.body.value.result.location),
                result_type,
            });
        }

        let mut definitions = Vec::new();
        let mut definition_maps = Vec::with_capacity(inputs.len());
        for (index, input) in inputs.iter().enumerate() {
            let Some(analysis) = &input.analysis else {
                definition_maps.push(Vec::new());
                continue;
            };
            let import_targets = input
                .imports
                .iter()
                .map(|import| (import.name.as_str(), ids[&import.target.key()]))
                .collect::<HashMap<_, _>>();
            let module = WorkspaceModuleId(index as u32);
            let mut map = Vec::with_capacity(analysis.hir.definitions().len());
            for definition in analysis.hir.definitions() {
                let id = DefinitionId(definitions.len() as u32);
                let ty = analysis
                    .definition_types
                    .get(&definition.id)
                    .map(|local| type_maps[index][local.index()]);
                definitions.push(Definition {
                    id,
                    module,
                    name: definition.name.clone(),
                    kind: definition.kind,
                    location: definition.location,
                    additional_locations: definition.additional_locations.clone(),
                    ty,
                    import_target: (definition.kind == DefinitionKind::Import)
                        .then(|| import_targets.get(definition.name.as_str()).copied())
                        .flatten(),
                });
                map.push(id);
            }
            definition_maps.push(map);
        }

        let mut references = Vec::new();
        let mut reference_maps = Vec::with_capacity(inputs.len());
        for (index, input) in inputs.iter().enumerate() {
            let Some(analysis) = &input.analysis else {
                reference_maps.push(Vec::new());
                continue;
            };
            let module = WorkspaceModuleId(index as u32);
            let mut map = Vec::with_capacity(analysis.hir.references().len());
            for reference in analysis.hir.references() {
                let id = ReferenceId(references.len() as u32);
                references.push(Reference {
                    id,
                    module,
                    name: reference.name.clone(),
                    location: reference.location,
                    definition: match reference.resolution {
                        HirResolution::Definition(definition) => {
                            Some(definition_maps[index][definition.index()])
                        }
                        HirResolution::External | HirResolution::Unresolved => None,
                    },
                    external: reference.resolution == HirResolution::External,
                });
                map.push(id);
            }
            reference_maps.push(map);
        }

        let mut expressions = Vec::new();
        for (index, input) in inputs.iter().enumerate() {
            let Some(analysis) = &input.analysis else {
                continue;
            };
            let module = WorkspaceModuleId(index as u32);
            for expression in analysis.hir.expressions() {
                expressions.push(WorkspaceExpression {
                    id: WorkspaceExpressionId(expressions.len() as u32),
                    module,
                    location: expression.location,
                    reference: expression
                        .reference
                        .map(|reference| reference_maps[index][reference.index()]),
                    ty: analysis
                        .expression_types
                        .get(&expression.id)
                        .map(|ty| type_maps[index][ty.index()]),
                });
            }
        }

        Self {
            sources,
            modules,
            definitions,
            references,
            expressions,
            types,
        }
    }
}

fn contains(range: Location, point: Location) -> bool {
    range.source == point.source
        && range.start <= point.start
        && (point.start < range.end || range.start == range.end && point.start == range.start)
}

fn merge_type_graph(
    module: &str,
    source: &TypeGraph,
    target: &mut WorkspaceTypeGraph,
) -> Vec<WorkspaceTypeId> {
    let mut mapped = vec![None; source.nodes().len()];
    for (id, _) in source.nodes() {
        merge_type_node(id, source, target, &mut mapped);
    }
    let mapped = mapped
        .into_iter()
        .map(|id| id.expect("all source type nodes are merged"))
        .collect::<Vec<_>>();
    for (name, id) in source.names() {
        target
            .names
            .insert(format!("{module}::{name}"), mapped[id.index()]);
    }
    mapped
}

fn merge_type_node(
    id: TypeId,
    source: &TypeGraph,
    target: &mut WorkspaceTypeGraph,
    mapped: &mut [Option<WorkspaceTypeId>],
) -> WorkspaceTypeId {
    if let Some(id) = mapped[id.index()] {
        return id;
    }
    let output = WorkspaceTypeId(target.nodes.len() as u32);
    target.nodes.push(WorkspaceTypeNode::Pending);
    mapped[id.index()] = Some(output);
    let map = |child, target: &mut WorkspaceTypeGraph, mapped: &mut [Option<WorkspaceTypeId>]| {
        merge_type_node(child, source, target, mapped)
    };
    let node = match source.node(id) {
        TypeNode::Pending => WorkspaceTypeNode::Pending,
        TypeNode::Ref(child) => WorkspaceTypeNode::Ref(map(*child, target, mapped)),
        TypeNode::Any => WorkspaceTypeNode::Any,
        TypeNode::Int => WorkspaceTypeNode::Int,
        TypeNode::Float => WorkspaceTypeNode::Float,
        TypeNode::String => WorkspaceTypeNode::String,
        TypeNode::Bytes => WorkspaceTypeNode::Bytes,
        TypeNode::Atom(atom) => WorkspaceTypeNode::Atom(atom.name().into()),
        TypeNode::Array(child) => WorkspaceTypeNode::Array(map(*child, target, mapped)),
        TypeNode::Tuple(children) => WorkspaceTypeNode::Tuple(
            children
                .iter()
                .map(|child| map(*child, target, mapped))
                .collect(),
        ),
        TypeNode::Struct(fields) => WorkspaceTypeNode::Struct(
            fields
                .iter()
                .map(|(name, child)| (name.clone(), map(*child, target, mapped)))
                .collect(),
        ),
        TypeNode::Enum(variants) => WorkspaceTypeNode::Enum(
            variants
                .iter()
                .map(|(name, child)| (name.clone(), child.map(|child| map(child, target, mapped))))
                .collect(),
        ),
        TypeNode::Union(children) => WorkspaceTypeNode::Union(
            children
                .iter()
                .map(|child| map(*child, target, mapped))
                .collect(),
        ),
        TypeNode::Function { parameters, result } => WorkspaceTypeNode::Function {
            parameters: parameters
                .iter()
                .map(|child| map(*child, target, mapped))
                .collect(),
            result: map(*result, target, mapped),
        },
    };
    target.nodes[output.index()] = node;
    output
}

#[derive(Clone, Debug)]
pub(crate) enum SemanticModuleTarget {
    Path(PathBuf),
    Core(String),
}

impl SemanticModuleTarget {
    fn key(&self) -> String {
        match self {
            Self::Path(path) => path.to_string_lossy().into_owned(),
            Self::Core(name) => name.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SemanticImport {
    pub name: String,
    pub location: Location,
    pub target: SemanticModuleTarget,
}

#[derive(Clone, Debug)]
pub(crate) struct SemanticModuleInput {
    pub key: String,
    pub path: Option<PathBuf>,
    pub kind: WorkspaceModuleKind,
    pub source: Option<SourceId>,
    pub program: Option<Program>,
    pub analysis: Option<Analysis>,
    pub imports: Vec<SemanticImport>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Engine, EngineConfig, Quota, TextRange};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture_dir() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("xl-semantic-test-{unique}"));
        fs::create_dir(&path).unwrap();
        path
    }

    fn engine() -> Engine {
        Engine::new(EngineConfig {
            module_quota: Quota::with_fuel(1_000_000),
            session_quota: Quota::with_fuel(1_000_000),
        })
    }

    #[test]
    fn indexes_workspace_modules_scopes_types_and_recursive_graphs() {
        let directory = fixture_dir();
        let model = directory.join("model.xl");
        let data = directory.join("data.json");
        let main = directory.join("main.xl");
        fs::write(
            &model,
            "@struct type Node = {children: Array(Node)}; {Node: Node}",
        )
        .unwrap();
        fs::write(&data, "{\"value\":1}").unwrap();
        fs::write(
            &main,
            "import model from \"./model.xl\";\n\
             import data from \"./data.json\";\n\
             let f = fn(x) { let y = x; y };\n\
             let count = 1 + 2;\n\
             {model: model, data: data, f: f, count: count}",
        )
        .unwrap();

        let loaded = engine().load_module(&main, BTreeMap::new()).unwrap();
        let snapshot = &loaded.workspace;
        assert_eq!(
            snapshot
                .modules()
                .iter()
                .filter(|module| module.kind != WorkspaceModuleKind::Core)
                .count(),
            3
        );
        let main_module = snapshot
            .module_by_path(&fs::canonicalize(&main).unwrap())
            .unwrap();
        let model_import = main_module
            .imports
            .iter()
            .find(|import| import.name == "model")
            .unwrap();
        assert_eq!(
            snapshot
                .module(model_import.target)
                .unwrap()
                .path
                .as_deref(),
            Some(fs::canonicalize(&model).unwrap().as_path())
        );

        let x = snapshot
            .definitions()
            .iter()
            .find(|definition| definition.name == "x")
            .unwrap();
        let x_use = snapshot
            .references()
            .iter()
            .find(|reference| reference.name == "x")
            .unwrap();
        assert_eq!(x_use.definition, Some(x.id));
        let y = snapshot
            .definitions()
            .iter()
            .find(|definition| definition.name == "y")
            .unwrap();
        assert_eq!(snapshot.references_of(y.id).len(), 1);

        let node = snapshot
            .definitions()
            .iter()
            .find(|definition| definition.name == "Node")
            .unwrap();
        let node_type = node.ty.unwrap();
        let shown = snapshot.types().display(node_type).unwrap();
        assert!(shown.contains("children: Array<"), "{shown}");
        assert!(shown.contains("Node"), "{shown}");
        assert_eq!(
            snapshot.type_at(Location::new(
                node.location.source,
                TextRange::at(node.location.start),
            )),
            Some(node_type)
        );
        let node_reference = snapshot
            .references()
            .iter()
            .find(|reference| reference.definition == Some(node.id))
            .unwrap();
        assert_eq!(
            snapshot.type_at(Location::new(
                node_reference.location.source,
                TextRange::at(node_reference.location.start),
            )),
            Some(node_type),
            "resolved type references must prefer the promoted definition root"
        );
        assert!(
            snapshot
                .exports_of(main_module.id)
                .iter()
                .any(|item| item.name == "f")
        );
        assert!(
            snapshot
                .expressions()
                .iter()
                .filter(|expression| expression.module == main_module.id)
                .all(|expression| expression.ty.is_some())
        );
        let main_source = snapshot.sources().get(main_module.source.unwrap());
        let literal = u32::try_from(main_source.text.find("1 + 2").unwrap()).unwrap();
        let expression = snapshot
            .expression_at(Location::new(main_source.id(), TextRange::at(literal)))
            .unwrap();
        assert_eq!(
            snapshot.types().node(expression.ty.unwrap()),
            Some(&WorkspaceTypeNode::Int)
        );

        fs::remove_dir_all(directory).unwrap();
    }
}
