use crate::id::{Id, IdPack, IdUnpack, IdFmt};
use crate::liveerror::{LiveError, LiveFileError, LiveErrorOrigin};
use makepad_live_derive::*;
use crate::livedocument::LiveDocument;
use crate::livedocument::LiveScopeTarget;
use crate::livedocument::LiveScopeItem;
use crate::livenode::LiveNode;
use crate::livenode::LiveValue;
use crate::liveparser::LiveParser;
use crate::id::FileId;
use crate::id::LocalNodePtr;
use crate::id::FullNodePtr;
use crate::token::TokenId;
use crate::token::Token;
use crate::span::Span;
use std::collections::HashMap;
use std::collections::HashSet;
use crate::lex::lex;
use std::fmt;

#[derive(Debug)]
pub struct LiveFile {
    pub crate_module: CrateModule,
    pub file: String,
    pub source: String,
    pub document: LiveDocument,
}

#[derive(Clone, Eq, Hash, Debug, Copy, PartialEq)]
pub struct CrateModule(pub Id, pub Id);

impl fmt::Display for CrateModule {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}::{}", self.0, self.1)
    }
}

#[derive(Default, Debug)]
pub struct LiveRegistry {
    pub file_ids: HashMap<String, FileId>,
    pub crate_module_to_file_id: HashMap<CrateModule, FileId>,
    pub live_files: Vec<LiveFile>,
    pub dep_order: Vec<(CrateModule, TokenId)>,
    pub dep_graph: HashMap<CrateModule, HashSet<CrateModule >>, // this contains all the dependencies a crate has
    pub expanded: Vec<LiveDocument >,
}


impl LiveRegistry {

     pub fn resolve_ptr(&self, full_ptr:FullNodePtr)->(&LiveDocument,&LiveNode){
        let doc = &self.expanded[full_ptr.file_id.to_index()];
        (doc,&doc.resolve_ptr(full_ptr.local_ptr))
    }
    
    pub fn live_error_to_live_file_error(&self, live_error:LiveError)->LiveFileError{
        let live_file = &self.live_files[live_error.span.file_id().to_index()];
        live_error.to_live_file_error(&live_file.file, &live_file.source)
    }
    
    pub fn is_baseclass(id: IdPack) -> bool {
        id == id_pack!(Component) || id == id_pack!(Enum) || id == id_pack!(Struct) || id == id_pack!(Shader) || id == id_pack!(Variant)
    }
    
    pub fn find_enum_origin(&self, start: IdPack, lhs: IdPack) -> IdPack {
        match start.unpack() {
            IdUnpack::FullNodePtr(full_ptr) => {
                let doc = &self.expanded[full_ptr.file_id.to_index()];
                let node = &doc.nodes[full_ptr.local_ptr.level][full_ptr.local_ptr.index];
                match node.value {
                    LiveValue::IdPack(id) => {
                        return self.find_enum_origin(id, node.id_pack)
                    }
                    LiveValue::Class {class, ..} => {
                        return self.find_enum_origin(class, node.id_pack)
                    },
                    LiveValue::Call {target, ..} => {
                        return self.find_enum_origin(target, node.id_pack)
                    },
                    _ => ()
                }
            }
            _ => ()
        }
        lhs
    }
    
    pub fn find_full_node_ptr_from_ids(&self, crate_id: Id, module_id: Id, ids: &[Id]) -> Option<FullNodePtr> {
        let cm = CrateModule(crate_id, module_id);
        if let Some(file_id) = self.crate_module_to_file_id.get(&cm) {
            let exp = &self.expanded[file_id.to_index()];
            if let Some(local_ptr) = exp.scan_for_multi(ids) {
                let node = &exp.nodes[local_ptr.level][local_ptr.index];
                match node.value {
                    LiveValue::Class {..} => {
                        return Some(FullNodePtr {file_id: *file_id, local_ptr})
                    },
                    _ => ()
                }
            }
        }
        None
    }
    
    pub fn find_base_class_id(&self, class: IdPack)->Option<IdPack>{
        let mut class_iter = class;
        while let IdUnpack::FullNodePtr(full_ptr) = class_iter.unpack() {
            let (_, other_node) = self.resolve_ptr(full_ptr);
            if let LiveValue::Class {class, ..} = other_node.value {
                class_iter = class;
            }
            else {
                return None
            }
        }
        Some(class_iter)        
    }
    
    pub fn find_component_origin(&self, crate_id: Id, module_id: Id, ids: &[Id]) -> Option<(CrateModule, Id, FullNodePtr)> {
        let cm = CrateModule(crate_id, module_id);
        if let Some(file_id) = self.crate_module_to_file_id.get(&cm) {
            let exp = &self.expanded[file_id.to_index()];
            if let Some(ptr) = exp.scan_for_multi(ids) {
                let node = &exp.nodes[ptr.level][ptr.index];
                match node.value {
                    LiveValue::Class {class, ..} => {
                        // ok so this thing can be 'endpoint'
                        let mut class_iter = class;
                        let mut token_id_iter = node.token_id;
                        while let IdUnpack::FullNodePtr(full_ptr) = class_iter.unpack() {
                            let (_,other_node) = self.resolve_ptr(full_ptr);
                            //let other = &self.expanded[full_ptr.file_id.to_index()];
                            //let other_node = &other.nodes[full_ptr.local_ptr.level][full_ptr.local_ptr.index];
                            if let LiveValue::Class {class, ..} = other_node.value {
                                class_iter = class;
                                token_id_iter = other_node.token_id;
                            }
                            else {
                                return None
                            }
                        }
                        // alright we found 'token'
                        let exp = &self.expanded[token_id_iter.file_id.to_index()];
                        let file = &self.live_files[token_id_iter.file_id.to_index()];
                        // this thing needs to be a Component.
                        if class_iter != id_pack!(Component) {
                            return None;
                        }
                        let token_span = &exp.tokens[token_id_iter.token_id as usize - 2];
                        // ok now we have a live_file_id we can turn into crate_module and a token
                        let crate_module = file.crate_module;
                        if let Token::Ident(id) = token_span.token {
                            // lets get the factory
                            return Some((crate_module, id, FullNodePtr {file_id: *file_id, local_ptr: ptr}));
                        }
                        // now we can look this up in our
                    }
                    _ => ()
                }
            }
        }
        None
    }
    
    pub fn token_id_to_span(&self, token_id: TokenId) -> Span {
        self.live_files[token_id.file_id.to_index()].document.token_id_to_span(token_id)
    }
    
    pub fn find_crate_module_by_file_id(&self, scan_file_id: FileId) -> Option<CrateModule> {
        for (crate_module, file_id) in &self.crate_module_to_file_id {
            if *file_id == scan_file_id {
                return Some(*crate_module)
            }
        }
        return None
    }
    
    pub fn parse_live_file(&mut self, file: &str, crate_id: Id, module_id: Id, source: String) -> Result<FileId, LiveFileError> {
        
        let (is_new_file_id, file_id) = if let Some(file_id) = self.file_ids.get(file) {
            (false, *file_id)
        }
        else {
            let file_id = FileId::index(self.live_files.len());
            (true, file_id)
        };
        
        let lex_result = match lex(source.chars(), file_id) {
            Err(msg) => panic!("Lex error {}", msg),
            Ok(lex_result) => lex_result
        };
        
        let mut parser = LiveParser::new(&lex_result.tokens);
        
        let mut document = match parser.parse_live_document() {
            Err(msg) => panic!("Parse error {}", msg.to_live_file_error(file, &source)),
            Ok(ld) => ld
        };
        document.strings = lex_result.strings;
        document.tokens = lex_result.tokens;
        
        let own_crate_module = CrateModule(crate_id, module_id);
        
        if self.dep_order.iter().position( | v | v.0 == own_crate_module).is_none() {
            self.dep_order.push((own_crate_module, TokenId::default()));
        }
        else {
            // marks dependencies dirty recursively (removes the expanded version)
            fn mark_dirty(cm: CrateModule, registry: &mut LiveRegistry) {
                if let Some(id) = registry.crate_module_to_file_id.get(&cm) {
                    registry.expanded[id.to_index()].recompile = true;
                }
                //registry.expanded.remove(&cm);
                
                let mut dirty = Vec::new();
                for (cm_iter, hs) in &registry.dep_graph {
                    if hs.contains(&cm) { // this
                        dirty.push(*cm_iter);
                    }
                }
                for d in dirty {
                    mark_dirty(d, registry);
                }
            }
            mark_dirty(own_crate_module, self);
        }
        
        let mut dep_graph_set = HashSet::new();
        
        for (_, nodes) in document.nodes.iter().enumerate() {
            for node in nodes {
                match node.value {
                    LiveValue::Use {crate_module} => {
                        let crate_module = document.fetch_crate_module(crate_module, crate_id);
                        dep_graph_set.insert(crate_module);
                        let self_index = self.dep_order.iter().position( | v | v.0 == own_crate_module).unwrap();
                        if let Some(other_index) = self.dep_order.iter().position( | v | v.0 == crate_module) {
                            if other_index > self_index {
                                self.dep_order.remove(other_index);
                                self.dep_order.insert(self_index, (crate_module, node.token_id));
                            }
                        }
                        else {
                            self.dep_order.insert(self_index, (crate_module, node.token_id));
                        }
                        
                    }, // import
                    _ => {
                    }
                }
            }
        }
        self.dep_graph.insert(own_crate_module, dep_graph_set);
        
        let live_file = LiveFile {
            crate_module: own_crate_module,
            file: file.to_string(),
            source,
            document
        };
        self.crate_module_to_file_id.insert(own_crate_module, file_id);

        if is_new_file_id {
            self.file_ids.insert(file.to_string(), file_id);
            self.live_files.push(live_file);
            self.expanded.push(LiveDocument::new());
        }
        else {
            self.live_files[file_id.to_index()] = live_file;
            self.expanded[file_id.to_index()].recompile = true;
        }
        
        return Ok(file_id)
    }
    
    pub fn expand_all_documents(&mut self, errors: &mut Vec<LiveError>) {
        
        struct ScopeStack {
            stack: Vec<Vec<LiveScopeItem >>
        }
        
        impl ScopeStack {
            fn find_item(&self, id: Id) -> Option<LiveScopeTarget> {
                for items in self.stack.iter().rev() {
                    for item in items.iter().rev() {
                        if item.id == id {
                            return Some(item.target)
                        }
                    }
                }
                return None
            }
        }
        
        #[derive(Debug)]
        enum CopyRecurResult {
            IsClass {class: IdPack},
            Noop,
            Error
        }
        
        fn copy_recur(
            scope_stack: &mut ScopeStack,
            in_doc: Option<(&LiveDocument, FileId)>,
            out_doc: &mut LiveDocument,
            skip_level_id: IdPack,
            skip_level: usize,
            in_level: usize,
            out_level: usize,
            in_index: usize,
        ) -> CopyRecurResult {
            let node = if let Some((in_doc, _)) = in_doc {
                in_doc.nodes[in_level][in_index]
            }
            else {
                out_doc.nodes[in_level][in_index]
            };
            let node_id = if skip_level == in_level {
                skip_level_id
            }
            else {
                node.id_pack
            };
            
            fn clone_scope(in_doc: &LiveDocument, out_doc: &mut LiveDocument, scope_start:usize, scope_count: usize, in_file_id:FileId){
                for i in 0..scope_count {
                    let item = &in_doc.scopes[i + scope_start];
                    // if item is local, it is now 'remote'.
                    match item.target {
                        LiveScopeTarget::Local(local_ptr) => {
                            out_doc.scopes.push(LiveScopeItem {
                                id: item.id,
                                target: LiveScopeTarget::Full(
                                    FullNodePtr {
                                        file_id: in_file_id,
                                        local_ptr
                                    }
                                )
                            });
                        },
                        LiveScopeTarget::Full {..} => {
                            out_doc.scopes.push(*item);
                        }
                    }
                }                
            }
            
            match node.value {
                LiveValue::Call {target, node_start, node_count} => {
                    let out_start = out_doc.get_level_len(out_level + 1);
                    for i in 0..node_count {
                        copy_recur(scope_stack, in_doc, out_doc, skip_level_id, skip_level, in_level + 1, out_level + 1, i as usize + node_start as usize);
                    }
                    
                    out_doc.push_node(out_level, LiveNode {
                        token_id: node.token_id,
                        id_pack: node_id,
                        value: LiveValue::Call {
                            target: target,
                            node_start: out_start as u32,
                            node_count: node_count
                        }
                    });
                    return CopyRecurResult::Noop
                },
                LiveValue::Array {node_start, node_count} => {
                    let out_start = out_doc.get_level_len(out_level + 1);
                    for i in 0..node_count {
                        copy_recur(scope_stack, in_doc, out_doc, skip_level_id, skip_level, in_level + 1, out_level + 1, i as usize + node_start as usize);
                    }
                    out_doc.push_node(out_level, LiveNode {
                        token_id: node.token_id,
                        id_pack: node_id,
                        value: LiveValue::Array {
                            node_start: out_start as u32,
                            node_count: node_count
                        }
                    });
                    return CopyRecurResult::Noop
                },
                LiveValue::Object {node_start, node_count} => {
                    let out_start = out_doc.get_level_len(out_level + 1);
                    for i in 0..node_count {
                        copy_recur(scope_stack, in_doc, out_doc, skip_level_id, skip_level, in_level + 1, out_level + 1, i as usize + node_start as usize);
                    }
                    out_doc.push_node(out_level, LiveNode {
                        token_id: node.token_id,
                        id_pack: node_id,
                        value: LiveValue::Object {
                            node_start: out_start as u32,
                            node_count: node_count
                        }
                    });
                    return CopyRecurResult::Noop
                },
                LiveValue::Use {..} => { // no need to output there.
                }
                LiveValue::Class {class, node_start, node_count} => {
                    if class == id_pack!(Self) {
                        return CopyRecurResult::Noop
                    }
                    let out_start = out_doc.get_level_len(out_level + 1);
                    for i in 0..node_count {
                        copy_recur(scope_stack, in_doc, out_doc, skip_level_id, skip_level, in_level + 1, out_level + 1, i as usize + node_start as usize);
                    }
                    if skip_level != in_level {
                        out_doc.push_node(out_level, LiveNode {
                            token_id: node.token_id,
                            id_pack: node.id_pack,
                            value: LiveValue::Class {
                                class: class,
                                node_start: out_start as u32,
                                node_count: node_count
                            }
                        });
                    }
                    return CopyRecurResult::IsClass {class}
                },
                LiveValue::String {string_start, string_count} => {
                    let new_string_start = if let Some((in_doc, _)) = in_doc { // copy the string if its from another doc
                        let nsi = out_doc.strings.len();
                        for i in 0..string_count {
                            out_doc.strings.push(in_doc.strings[(i + string_start) as usize]);
                        }
                        nsi
                    }
                    else {
                        string_start as usize
                    };
                    out_doc.push_node(out_level, LiveNode {
                        token_id: node.token_id,
                        id_pack: node_id,
                        value: LiveValue::String {
                            string_start: new_string_start as u32,
                            string_count
                        }
                    });
                    return CopyRecurResult::Noop
                }
                LiveValue::Fn {token_start, token_count, scope_start, scope_count} => {
                    let (new_token_start, new_scope_start) = if let Some((in_doc, in_file_id)) = in_doc { // copy the string if its from another doc
                        let nts = out_doc.tokens.len();
                        let nss = out_doc.scopes.len();
                        for i in 0..(token_count as usize) {
                            out_doc.tokens.push(in_doc.tokens[i + token_start as usize]);
                        }
                        clone_scope(in_doc, out_doc, scope_start as usize, scope_count as usize, in_file_id);
                        (nts as u32, nss as u32)
                    }
                    else {
                        (token_start, scope_start)
                    };
                    out_doc.push_node(out_level, LiveNode {
                        token_id: node.token_id,
                        id_pack: node_id,
                        value: LiveValue::Fn {
                            token_start: new_token_start,
                            scope_start: new_scope_start,
                            token_count,
                            scope_count
                        }
                    });
                    return CopyRecurResult::Noop
                }
                LiveValue::VarDef {token_start, token_count, scope_start, scope_count} => {
                    let (new_token_start, new_scope_start) = if let Some((in_doc, in_file_id)) = in_doc { // copy the string if its from another doc
                        let nts = out_doc.tokens.len();
                        let nss = out_doc.scopes.len();
                        for i in 0..(token_count as usize) {
                            out_doc.tokens.push(in_doc.tokens[i + token_start as usize]);
                        }
                        clone_scope(in_doc, out_doc, scope_start as usize, scope_count as usize, in_file_id);
                        (nts as u32, nss as u32)
                    }
                    else {
                        (token_start, scope_start)
                    };
                    out_doc.push_node(out_level, LiveNode {
                        token_id: node.token_id,
                        id_pack: node_id,
                        value: LiveValue::VarDef {
                            token_start: new_token_start,
                            scope_start: new_scope_start,
                            token_count,
                            scope_count
                        }
                    });
                    return CopyRecurResult::Noop
                }
                LiveValue::ResourceRef {target} => {
                    let new_target = if let Some((in_doc, _)) = in_doc { // copy the string if its from another doc
                        out_doc.clone_multi_id(target, &in_doc.multi_ids)
                    }
                    else {
                        target
                    };
                    out_doc.push_node(out_level, LiveNode {
                        token_id: node.token_id,
                        id_pack: node_id,
                        value: LiveValue::ResourceRef {
                            target: new_target,
                        }
                    });
                    return CopyRecurResult::Noop
                }
                _ => {
                    out_doc.push_node(out_level, LiveNode {
                        token_id: node.token_id,
                        id_pack: node_id,
                        value: node.value
                    });
                    return CopyRecurResult::Noop
                }
            }
            return CopyRecurResult::Noop
        }
        
        fn write_or_add_node(scope_stack: &mut ScopeStack, errors: &mut Vec<LiveError>, out_doc: &mut LiveDocument, level: usize, node_start: usize, node_count: usize, in_doc: &LiveDocument, in_node: &LiveNode) {
            match out_doc.write_or_add_node(level, node_start, node_count, in_doc, in_node) {
                Err(err) => errors.push(err),
                Ok(Some(index)) => {
                    if scope_stack.stack.len() - 1 == level {
                        match in_node.id_pack.unpack() {
                            IdUnpack::Single(id) => {
                                scope_stack.stack[level].push(LiveScopeItem {
                                    id: id,
                                    target: LiveScopeTarget::Local(LocalNodePtr {level: level, index: index})
                                });
                            }
                            _ => ()
                        }
                    }
                }
                _ => ()
            }
        }
        
        fn resolve_id(
            resolve_id: IdPack,
            expanded: &Vec<LiveDocument >,
            token_id: TokenId,
            scope_stack: &ScopeStack,
            in_doc: &LiveDocument,
            out_doc: &mut LiveDocument,
            out_level: usize,
            out_start: usize,
        ) -> Result<(Option<FileId>, LocalNodePtr), LiveError> {
            match resolve_id.unpack() {
                IdUnpack::Multi {index: id_start, count: id_count} => {
                    let base = in_doc.multi_ids[id_start];
                    // base id can be Self or a scope target
                    if base == id!(Self) {
                        // lets find our sub id chain on self
                        let out_count = out_doc.get_level_len(out_level) - out_start;
                        match out_doc.scan_for_multi_for_expand(out_level, out_start, out_count, id_start, id_count, &in_doc.multi_ids,) {
                            Ok(found_node) => {
                                return Ok((None, found_node))
                            }
                            Err(message) => {
                                return Err(LiveError {
                                    origin: live_error_origin!(),
                                    span: out_doc.token_id_to_span(token_id),
                                    message
                                });
                            }
                        }
                    }
                    else if LiveRegistry::is_baseclass(IdPack::single(base)) {
                        return Err(LiveError {
                            origin: live_error_origin!(),
                            span: in_doc.token_id_to_span(token_id),
                            message: format!("Cannot use baseclass {}", base)
                        });
                    }
                    else {
                        match scope_stack.find_item(base) {
                            Some(LiveScopeTarget::Local(node_ptr)) => {
                                match &out_doc.nodes[node_ptr.level][node_ptr.index].value {
                                    LiveValue::Class {node_start, node_count, ..} => {
                                        match out_doc.scan_for_multi_for_expand(node_ptr.level + 1, *node_start as usize, *node_count as usize, id_start, id_count, &in_doc.multi_ids) {
                                            Ok(found_node) => {
                                                return Ok((None, found_node))
                                            }
                                            Err(message) => {
                                                return Err(LiveError {
                                                    origin: live_error_origin!(),
                                                    span: out_doc.token_id_to_span(token_id),
                                                    message
                                                });
                                            }
                                        }
                                    }
                                    _ => {
                                        return Err(LiveError {
                                            origin: live_error_origin!(),
                                            span: in_doc.token_id_to_span(token_id),
                                            message: format!("Property is not a class {} of {}", base, IdFmt::col(&in_doc.multi_ids, resolve_id))
                                        });
                                    }
                                }
                            }
                            Some(LiveScopeTarget::Full(full_ptr)) => {
                                let other_doc = &expanded[full_ptr.file_id.to_index()];
                                match &other_doc.nodes[full_ptr.local_ptr.level][full_ptr.local_ptr.index].value {
                                    LiveValue::Class {node_start, node_count, ..} => {
                                        match other_doc.scan_for_multi_for_expand(full_ptr.local_ptr.level + 1, *node_start as usize, *node_count as usize, id_start, id_count, &in_doc.multi_ids) {
                                            Ok(found_node) => {
                                                return Ok((Some(full_ptr.file_id), found_node))
                                            }
                                            Err(message) => {
                                                return Err(LiveError {
                                                    origin: live_error_origin!(),
                                                    span: out_doc.token_id_to_span(token_id),
                                                    message
                                                });
                                            }
                                        }
                                    }
                                    _ => {
                                        return Err(LiveError {
                                            origin: live_error_origin!(),
                                            span: in_doc.token_id_to_span(token_id),
                                            message: format!("Property is not a class {} of {}", base, IdFmt::col(&in_doc.multi_ids, resolve_id))
                                        });
                                    }
                                }
                            }
                            None => { // scope item not found, error
                                return Err(LiveError {
                                    origin: live_error_origin!(),
                                    span: in_doc.token_id_to_span(token_id),
                                    message: format!("Cannot find item on scope: {} of {}", base, IdFmt::col(&in_doc.multi_ids, resolve_id))
                                });
                            }
                        }
                    }
                }
                IdUnpack::Single(id) if !LiveRegistry::is_baseclass(IdPack::single(id)) => {
                    match scope_stack.find_item(id) {
                        Some(LiveScopeTarget::Local(local_ptr)) => {
                            return Ok((None, local_ptr));
                        }
                        Some(LiveScopeTarget::Full(full_ptr)) => {
                            return Ok((Some(full_ptr.file_id), full_ptr.local_ptr));
                        }
                        _ => {}
                    }
                }
                _ => ()
            }
            return Err(LiveError {
                origin: live_error_origin!(),
                span: in_doc.token_id_to_span(token_id),
                message: format!("Cannot find item on scope: {}", resolve_id)
            });
        }
        
        // This should we win me some kind of award. Absolute worst programmer in recent history or something like it.
        fn walk_node(
            expanded: &Vec<LiveDocument >,
            crate_module_to_file_id: &HashMap<CrateModule, FileId>,
            in_crate: Id,
            in_file_id: FileId,
            errors: &mut Vec<LiveError>,
            scope_stack: &mut ScopeStack,
            in_doc: &LiveDocument,
            out_doc: &mut LiveDocument,
            in_level: usize,
            out_level: usize,
            in_node_index: usize,
            out_start: usize,
            out_count: usize
        ) {
            let node = &in_doc.nodes[in_level][in_node_index];
            
            //let (row,col) = byte_to_row_col(node.span.start(), &ld.source);
            //let _ = write!(f, "/*{},{} {}*/", row+1, col, node.span.len());
            match node.value {
                LiveValue::String {..} => write_or_add_node(scope_stack, errors, out_doc, out_level, out_start, out_count, in_doc, node),
                LiveValue::Bool(_) => write_or_add_node(scope_stack, errors, out_doc, out_level, out_start, out_count, in_doc, node),
                LiveValue::Int(_) => write_or_add_node(scope_stack, errors, out_doc, out_level, out_start, out_count, in_doc, node),
                LiveValue::Float(_) => write_or_add_node(scope_stack, errors, out_doc, out_level, out_start, out_count, in_doc, node),
                LiveValue::Color(_) => write_or_add_node(scope_stack, errors, out_doc, out_level, out_start, out_count, in_doc, node),
                LiveValue::Vec2(_) => write_or_add_node(scope_stack, errors, out_doc, out_level, out_start, out_count, in_doc, node),
                LiveValue::Vec3(_) => write_or_add_node(scope_stack, errors, out_doc, out_level, out_start, out_count, in_doc, node),
                LiveValue::IdPack(id_value) => {
                    // lets resolve ID
                    let out_index = out_doc.get_level_len(out_level);
                    write_or_add_node(scope_stack, errors, out_doc, out_level, out_start, out_count, in_doc, node);
                    if id_value != id_pack!(Self) && !LiveRegistry::is_baseclass(id_value) {
                        let result = resolve_id(
                            id_value,
                            expanded,
                            node.token_id,
                            scope_stack,
                            in_doc,
                            out_doc,
                            out_level,
                            out_start,
                        );
                        match result {
                            Ok((None, found_node)) => {
                                let new_id = IdPack::node_ptr(in_file_id, found_node);
                                let written_node = &mut out_doc.nodes[out_level][out_index];
                                if let LiveValue::IdPack(id) = &mut written_node.value {
                                    *id = new_id;
                                }
                            }
                            Ok((Some(found_file_id), found_node)) => {
                                let new_id = IdPack::node_ptr(found_file_id, found_node);
                                let written_node = &mut out_doc.nodes[out_level][out_index];
                                if let LiveValue::IdPack(id) = &mut written_node.value {
                                    *id = new_id;
                                }
                            }
                            Err(err) => {
                                errors.push(err);
                                return
                            }
                        }
                    }
                    
                }
                LiveValue::Call {target, node_start, node_count} => {
                    let new_node_start = out_doc.get_level_len(out_level + 1);
                    for i in 0..node_count {
                        walk_node(expanded, crate_module_to_file_id, in_crate, in_file_id, errors, scope_stack, in_doc, out_doc, in_level + 1, out_level + 1, i as usize + node_start as usize, out_start, 0);
                    }
                    let new_node = LiveNode {
                        token_id: node.token_id,
                        id_pack: node.id_pack,
                        value: LiveValue::Call {
                            target,
                            node_start: new_node_start as u32,
                            node_count: node_count
                        }
                    };
                    let out_index = out_doc.get_level_len(out_level);
                    write_or_add_node(scope_stack, errors, out_doc, out_level, out_start, out_count, in_doc, &new_node);
                    if target != id_pack!(Self) && !LiveRegistry::is_baseclass(target) {
                        let result = resolve_id(
                            target,
                            expanded,
                            node.token_id,
                            scope_stack,
                            in_doc,
                            out_doc,
                            out_level,
                            out_start,
                        );
                        match result {
                            Ok((None, found_node)) => {
                                // found node has to be a call too
                                let f_n = &out_doc.nodes[found_node.level][found_node.index];
                                if let LiveValue::Call {..} = f_n.value {}
                                else {
                                    errors.push(LiveError {
                                        origin: live_error_origin!(),
                                        span: in_doc.token_id_to_span(node.token_id),
                                        message: format!("Target not a call {}", IdFmt::col(&in_doc.multi_ids, node.id_pack))
                                    });
                                    return
                                }
                                let new_id = IdPack::node_ptr(in_file_id, found_node);
                                let written_node = &mut out_doc.nodes[out_level][out_index];
                                if let LiveValue::Call {target, ..} = &mut written_node.value {
                                    *target = new_id;
                                }
                            }
                            Ok((Some(found_file_id), found_node)) => {
                                let f_n = &expanded[found_file_id.to_index()].nodes[found_node.level][found_node.index];
                                if let LiveValue::Call {..} = f_n.value {}
                                else {
                                    errors.push(LiveError {
                                        origin: live_error_origin!(),
                                        span: in_doc.token_id_to_span(node.token_id),
                                        message: format!("Target not a call {}", IdFmt::col(&in_doc.multi_ids, node.id_pack))
                                    });
                                    return
                                }
                                let new_id = IdPack::node_ptr(found_file_id, found_node);
                                let written_node = &mut out_doc.nodes[out_level][out_index];
                                if let LiveValue::Call {target, ..} = &mut written_node.value {
                                    *target = new_id;
                                }
                                // store pointer
                            }
                            Err(err) => {
                                errors.push(err);
                                return
                            }
                        }
                    }
                },
                LiveValue::Array {node_start, node_count} => { // normal array
                    let shifted_out_level = if node.id_pack.is_multi() {
                        let (_start, len) = node.id_pack.unwrap_multi();
                        out_level + (len - 1)
                    }
                    else {
                        out_level
                    };
                    let new_node_start = out_doc.get_level_len(shifted_out_level + 1);
                    for i in 0..node_count {
                        walk_node(expanded, crate_module_to_file_id, in_crate, in_file_id, errors, scope_stack, in_doc, out_doc, in_level + 1, shifted_out_level + 1, i as usize + node_start as usize, out_start, 0);
                    }
                    let new_node = LiveNode {
                        token_id: node.token_id,
                        id_pack: node.id_pack,
                        value: LiveValue::Array {
                            node_start: new_node_start as u32,
                            node_count: node_count as u32
                        }
                    };
                    write_or_add_node(scope_stack, errors, out_doc, out_level, out_start, out_count, in_doc, &new_node);
                },
                LiveValue::Object {node_start, node_count} => {
                    let shifted_out_level = if node.id_pack.is_multi() {
                        let (_start, len) = node.id_pack.unwrap_multi();
                        out_level + (len - 1)
                    }
                    else {
                        out_level
                    };
                    let new_node_start = out_doc.get_level_len(shifted_out_level + 1);
                    for i in 0..node_count {
                        walk_node(expanded, crate_module_to_file_id, in_crate, in_file_id, errors, scope_stack, in_doc, out_doc, in_level + 1, shifted_out_level + 1, i as usize + node_start as usize, out_start, 0);
                    }
                    let new_node = LiveNode {
                        token_id: node.token_id,
                        id_pack: node.id_pack,
                        value: LiveValue::Object {
                            node_start: new_node_start as u32,
                            node_count: node_count as u32
                        }
                    };
                    write_or_add_node(scope_stack, errors, out_doc, out_level, out_start, out_count, in_doc, &new_node);
                },
                LiveValue::Fn {token_start, token_count, ..} => {
                    // we should store the scopestack here so the shader compiler can find symbols.
                    let new_scope_start = out_doc.scopes.len();
                    for i in 0..scope_stack.stack.len() {
                        let scope = &scope_stack.stack[i];
                        for j in 0..scope.len() {
                            out_doc.scopes.push(scope[j]);
                        }
                    }
                    let new_node = LiveNode {
                        token_id: node.token_id,
                        id_pack: node.id_pack,
                        value: LiveValue::Fn {
                            token_start,
                            token_count,
                            scope_start: new_scope_start as u32,
                            scope_count: (out_doc.scopes.len() - new_scope_start) as u16
                        }
                    };
                    write_or_add_node(scope_stack, errors, out_doc, out_level, out_start, out_count, in_doc, &new_node);
                },
                LiveValue::VarDef {token_start, token_count, ..} => {
                    // we should store the scopestack here so the shader compiler can find symbols.
                    let new_scope_start = out_doc.scopes.len();
                    for i in 0..scope_stack.stack.len() {
                        let scope = &scope_stack.stack[i];
                        for j in 0..scope.len() {
                            out_doc.scopes.push(scope[j]);
                        }
                    }
                    let new_node = LiveNode {
                        token_id: node.token_id,
                        id_pack: node.id_pack,
                        value: LiveValue::VarDef {
                            token_start,
                            token_count,
                            scope_start: new_scope_start as u32,
                            scope_count: (out_doc.scopes.len() - new_scope_start) as u16
                        }
                    };
                    write_or_add_node(scope_stack, errors, out_doc, out_level, out_start, out_count, in_doc, &new_node);
                },
                LiveValue::ResourceRef {target} => {
                    // we should store the scopestack here so the shader compiler can find symbols.
                    let new_node = LiveNode {
                        token_id: node.token_id,
                        id_pack: node.id_pack,
                        value: LiveValue::ResourceRef {
                            target//:out_doc.clone_multi_id(target, &in_doc.multi_ids),
                        }
                    };
                    write_or_add_node(scope_stack, errors, out_doc, out_level, out_start, out_count, in_doc, &new_node);
                },
                LiveValue::Use {crate_module} => { // import things on the scope from Use
                    let crate_module = in_doc.fetch_crate_module(crate_module, in_crate);
                    let file_id = crate_module_to_file_id.get(&crate_module).unwrap();
                    let other_doc = &expanded[file_id.to_index()];
                    
                    match node.id_pack.unpack() {
                        IdUnpack::Empty => { // its a wildcard
                            let nodes = &other_doc.nodes[0];
                            for i in 0..nodes.len() {
                                let id = nodes[i].id_pack;
                                match id.unpack() {
                                    IdUnpack::Single(id) => {
                                        scope_stack.stack[out_level].push(LiveScopeItem {
                                            id,
                                            target: LiveScopeTarget::Full(
                                                FullNodePtr {
                                                    file_id: *file_id,
                                                    local_ptr: LocalNodePtr {level: 0, index: i}
                                                }
                                            )
                                        });
                                    }
                                    _ => ()
                                }
                            }
                        },
                        IdUnpack::Single(_) => {
                            let nodes = &other_doc.nodes[0];
                            let mut found = false;
                            for i in 0..nodes.len() {
                                if nodes[i].id_pack == node.id_pack { // found it
                                    match node.id_pack.unpack() {
                                        IdUnpack::Single(id) => {
                                            scope_stack.stack[out_level].push(LiveScopeItem {
                                                id: id,
                                                target: LiveScopeTarget::Full(
                                                    FullNodePtr {
                                                        file_id: *file_id,
                                                        local_ptr: LocalNodePtr {level: 0, index: i}
                                                    }
                                                )
                                            });
                                        }
                                        _ => ()
                                    }
                                    found = true;
                                    break;
                                }
                            }
                            if !found {
                                errors.push(LiveError {
                                    origin: live_error_origin!(),
                                    span: in_doc.token_id_to_span(node.token_id),
                                    message: format!("Cannot find import {}", IdFmt::col(&in_doc.multi_ids, node.id_pack))
                                });
                            }
                        }
                        IdUnpack::Multi {index, count} => {
                            // lets validate if it exists!
                            let mut node_start = 0 as usize;
                            let mut node_count = other_doc.nodes[0].len();
                            for level in 0..count {
                                let id = in_doc.multi_ids[level + index];
                                if id.is_empty() {
                                    if level != count - 1 { // cant appear except at end
                                        panic!()
                                    }
                                    for i in 0..node_count {
                                        //let other_node = &other_doc.nodes[level][i + node_start];
                                        match node.id_pack.unpack() {
                                            IdUnpack::Single(id) => {
                                                scope_stack.stack[out_level].push(LiveScopeItem {
                                                    id: id,
                                                    target: LiveScopeTarget::Full(
                                                        FullNodePtr {
                                                            file_id: *file_id,
                                                            local_ptr: LocalNodePtr {level, index: i + node_start}
                                                        }
                                                    )
                                                });
                                            }
                                            _ => ()
                                        }
                                    }
                                }
                                else {
                                    let mut found = false;
                                    for i in 0..node_count {
                                        let other_node = &other_doc.nodes[level][i + node_start];
                                        if level == count - 1 {
                                            if IdPack::single(id) == other_node.id_pack {
                                                scope_stack.stack[out_level].push(LiveScopeItem {
                                                    id: id,
                                                    target: LiveScopeTarget::Full(
                                                        FullNodePtr {
                                                            file_id: *file_id,
                                                            local_ptr: LocalNodePtr {level, index: i + node_start}
                                                        }
                                                    )
                                                });
                                                found = true;
                                                break;
                                            }
                                        }
                                        if IdPack::single(id) == other_node.id_pack {
                                            match other_node.value {
                                                LiveValue::Class {node_start: ns, node_count: nc, ..} => {
                                                    node_start = ns as usize;
                                                    node_count = nc as usize;
                                                    found = true;
                                                    break;
                                                },
                                                _ => {
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                    if !found {
                                        errors.push(LiveError {
                                            origin: live_error_origin!(),
                                            span: in_doc.token_id_to_span(node.token_id),
                                            message: format!("Use path not found {}", IdFmt::col(&in_doc.multi_ids, node.id_pack))
                                        });
                                    }
                                }
                            }
                        }
                        _ => {
                            errors.push(LiveError {
                                origin: live_error_origin!(),
                                span: in_doc.token_id_to_span(node.token_id),
                                message: format!("Node type invalid {}", IdFmt::col(&in_doc.multi_ids, node.id_pack))
                            });
                        }
                    }
                }
                LiveValue::Class {class, node_start, node_count} => {
                    //let out_index = out_doc.get_level_len(out_level);
                    scope_stack.stack.push(Vec::new());
                    // if our id is a multi-id, write the clone at the correct level
                    let shifted_out_level = if node.id_pack.is_multi() {
                        let (_start, len) = node.id_pack.unwrap_multi();
                        out_level + (len - 1)
                    }
                    else {
                        out_level
                    };
                    
                    let new_out_start = out_doc.get_level_len(shifted_out_level + 1);
                    
                    // result values of the below scan
                    let mut copy_result = CopyRecurResult::IsClass {class};
                    let mut value_ptr = None;
                    let mut other_file_id = None;
                    
                    if class == id_pack!(Self) {
                        // recursively clone self
                        for i in out_start..out_doc.get_level_len(out_level) {
                            copy_recur(scope_stack, None, out_doc, node.id_pack, 0, out_level, shifted_out_level + 1, i);
                        }
                    }
                    else if !LiveRegistry::is_baseclass(class) {
                        let result = resolve_id(
                            class,
                            expanded,
                            node.token_id,
                            scope_stack,
                            in_doc,
                            out_doc,
                            out_level,
                            out_start,
                        );
                        match result {
                            Ok((None, found_node)) => {
                                copy_result = copy_recur(scope_stack, None, out_doc, node.id_pack, found_node.level, found_node.level, shifted_out_level, found_node.index);
                                value_ptr = Some(found_node);
                            }
                            Ok((Some(found_file_id), found_node)) => {
                                let other_doc = &expanded[found_file_id.to_index()];
                                other_file_id = Some(found_file_id);
                                copy_result = copy_recur(scope_stack, Some((other_doc, found_file_id)), out_doc, node.id_pack, found_node.level, found_node.level, shifted_out_level, found_node.index);
                                value_ptr = Some(found_node);
                            }
                            Err(err) => {
                                errors.push(err);
                                return
                            }
                        }
                    }
                    
                    if let CopyRecurResult::IsClass {..} = copy_result {}
                    else if node_count >0 {
                        errors.push(LiveError {
                            origin: live_error_origin!(),
                            span: in_doc.token_id_to_span(node.token_id),
                            message: format!("Cannot override items in non-class: {}", IdFmt::col(&in_doc.multi_ids, class))
                        });
                        return
                    }
                    
                    match copy_result {
                        CopyRecurResult::IsClass {class} => {
                            
                            let new_class_id = if let Some(other_file_id) = other_file_id {
                                if let Some(value_ptr) = value_ptr {
                                    IdPack::node_ptr(other_file_id, value_ptr)
                                }
                                else {
                                    class
                                }
                            }
                            else {
                                if let Some(value_ptr) = value_ptr {
                                    IdPack::node_ptr(in_file_id, value_ptr)
                                }
                                else {
                                    class
                                }
                            };
                            
                            let new_out_count = out_doc.get_level_len(shifted_out_level + 1) - new_out_start;
                            for i in 0..node_count {
                                walk_node(expanded, crate_module_to_file_id, in_crate, in_file_id, errors, scope_stack, in_doc, out_doc, in_level + 1, shifted_out_level + 1, i as usize + node_start as usize, new_out_start, new_out_count);
                            }
                            let new_out_count = out_doc.get_level_len(shifted_out_level + 1) - new_out_start;
                            
                            let new_node = LiveNode {
                                token_id: node.token_id,
                                id_pack: node.id_pack,
                                value: LiveValue::Class {
                                    class: new_class_id,
                                    node_start: new_out_start as u32,
                                    node_count: new_out_count as u16
                                }
                            };
                            scope_stack.stack.pop();
                            write_or_add_node(scope_stack, errors, out_doc, out_level, out_start, out_count, in_doc, &new_node);
                        }
                        CopyRecurResult::Noop | CopyRecurResult::Error => {
                            scope_stack.stack.pop();
                        }
                    }
                }
            }
        }
        
        for (crate_module, token_id) in &self.dep_order {
            let file_id = if let Some(file_id) = self.crate_module_to_file_id.get(crate_module) {
                file_id
            }
            else {
                // ok so we have a token_id. now what.
                errors.push(LiveError {
                    origin: live_error_origin!(),
                    span: self.token_id_to_span(*token_id),
                    message: format!("Cannot find dependency: {}::{}", crate_module.0, crate_module.1)
                });
                continue
            };
            
            if !self.expanded[file_id.to_index()].recompile {
                continue;
            }
            let live_file = &self.live_files[file_id.to_index()];
            let in_doc = &live_file.document;
            
            let mut out_doc = LiveDocument::new();
            std::mem::swap(&mut out_doc, &mut self.expanded[file_id.to_index()]);
            out_doc.restart_from(&in_doc);
            
            let mut scope_stack = ScopeStack {
                stack: vec![Vec::new()]
            };
            let len = in_doc.nodes[0].len();
            
            for i in 0..len {
                walk_node(&self.expanded, &self.crate_module_to_file_id, crate_module.0, *file_id, errors, &mut scope_stack, in_doc, &mut out_doc, 0, 0, i, 0, 0);
            }
            
            out_doc.recompile = false;
            
            std::mem::swap(&mut out_doc, &mut self.expanded[file_id.to_index()]);
        }
    }
}