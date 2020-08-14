use crate::{
    bar::{self, alignment::Alignment, component::Component},
    config::{update_channel::UpdateChannel, Rule, WorkspaceSetting, Config},
    keybindings::{keybinding::Keybinding, keybinding_type::KeybindingType},
};
use log::error;
use regex::Regex;
use rhai::{Array, Dynamic, Engine, Map, ParseError, Scope};
use std::{path::PathBuf, str::FromStr, rc::Rc, cell::RefCell};

#[macro_use]
mod macros;

fn add_keybinding(engine: &Engine, scope: &mut Scope, key: String, binding: KeybindingType) {
    let mut kb = Keybinding::from_str(&key).unwrap();

    kb.typ = binding;
    kb.mode = scope.get_value::<Option<String>>("__mode").unwrap();

    scope.set_value("__new_keybinding", kb);

    engine
        .consume_with_scope(scope, "__keybindings.push(__new_keybinding);")
        .unwrap();
}

fn add_rule(engine: &Engine, scope: &mut Scope, rule: Rule) {
    scope.set_value("__new_rule", rule);

    engine
        .consume_with_scope(scope, "__rules.push(__new_rule);")
        .unwrap();
}

fn set(engine: &Engine, scope: &mut Scope, key: String, val: Dynamic) -> Result<(), String> {
    scope.set_value("__new_set_key", key);
    scope.set_value("__new_set_val", val);

    engine
        .consume_with_scope(scope, "__set[__new_set_key] = __new_set_val;")
        .map_err(|x| x.to_string())
}

pub fn init(engine: &mut Engine, config: &mut Rc<RefCell<Config>>) -> Result<(), Box<ParseError>> {
    let cfg = config.clone();
    engine.register_custom_syntax(
        &["bind", "$expr$", "$expr$"], // the custom syntax
        0, // the number of new variables declared within this custom syntax
        move |engine, ctx, scope, inputs| {
            let key = get_string!(engine, ctx, scope, inputs, 0);
            let binding = get_type!(engine, ctx, scope, inputs, 1, KeybindingType);
            let mut kb = Keybinding::from_str(&key).unwrap();

            kb.typ = binding;
            kb.mode = scope.get_value::<Option<String>>("__mode").unwrap();

            cfg.borrow_mut().keybindings.push(kb);

            Ok(().into())
        },
    )?;

    let cfg = config.clone();
    engine.register_custom_syntax(
        &["bind_range", "$expr$", "$expr$", "$expr$", "$ident$"], // the custom syntax
        0, // the number of new variables declared within this custom syntax
        move |engine, ctx, scope, inputs| {
            let from = get_int!(engine, ctx, scope, inputs, 0);
            let to = get_int!(engine, ctx, scope, inputs, 1);
            let modifier = get_string!(engine, ctx, scope, inputs, 2);
            let binding_name = get_variable_name!(inputs, 3);

            for i in from..to + 1 {
                if i == 11 {
                    break;
                }

                let key = if i == 10 {
                    format!("{}+{}", modifier, 0)
                } else {
                    format!("{}+{}", modifier, i)
                };

                let binding: KeybindingType =
                    engine.eval_expression(&format!("{}({})", binding_name, i))?;

                let mut kb = Keybinding::from_str(&key).unwrap();

                kb.typ = binding;
                kb.mode = scope.get_value::<Option<String>>("__mode").unwrap();

                cfg.borrow_mut().keybindings.push(kb);
            }

            Ok(().into())
        },
    )?;

    engine.register_custom_syntax(
        &["execute", "$expr$"], // the custom syntax
        0,                      // the number of new variables declared within this custom syntax
        |engine, ctx, scope, inputs| {
            let cwd = scope
                .get_value::<String>("__cwd")
                .ok_or("Failed to get __cwd")?;
            let file_name = get_string!(engine, ctx, scope, inputs, 0) + ".nog";

            let mut path = PathBuf::new();

            path.push(cwd);
            path.push(file_name);

            engine.consume_file_with_scope(scope, path)?;

            Ok(().into())
        },
    )?;

    let cfg = config.clone();
    engine.register_custom_syntax(
        &["bar", "$expr$"], // the custom syntax
        0,                  // the number of new variables declared within this custom syntax
        move |engine, ctx, scope, inputs| {
            let settings = get_map!(engine, ctx, scope, inputs, 0);

            for (key, val) in settings {
                if key.to_string() == "components" {
                    let map = val.cast::<Map>();

                    for (key, val) in map {
                        let key = key.to_string();
                        let components = val.cast::<Array>();

                        let alignment = match key.as_str() {
                            "left" => Some(Alignment::Left),
                            "center" => Some(Alignment::Center),
                            "right" => Some(Alignment::Right),
                            _ => None,
                        };

                        if let Some(alignment) = alignment {
                            for v in components {
                                let component = v.cast::<Component>();
                                bar::add_component(alignment, component);
                            }
                        }
                    }
                } else {
                    set(engine, scope, format!("app_bar_{}", key), val.clone())?;
                }
            }

            Ok(().into())
        },
    )?;

    engine.register_custom_syntax(
        &["set", "$ident$", "$expr$"], // the custom syntax
        0, // the number of new variables declared within this custom syntax
        |engine, ctx, scope, inputs| {
            let key = get_variable_name!(inputs, 0);
            let value = get_dynamic!(engine, ctx, scope, inputs, 1);

            set(engine, scope, key, value)?;

            Ok(().into())
        },
    )?;

    engine.register_custom_syntax(
        &["enable", "$ident$"], // the custom syntax
        0,                      // the number of new variables declared within this custom syntax
        |engine, _ctx, scope, inputs| {
            let key = get_variable_name!(inputs, 0);

            set(engine, scope, key, true.into())?;

            Ok(().into())
        },
    )?;

    engine.register_custom_syntax(
        &["disable", "$ident$"], // the custom syntax
        0,                       // the number of new variables declared within this custom syntax
        |engine, _ctx, scope, inputs| {
            let key = get_variable_name!(inputs, 0);

            set(engine, scope, key, false.into())?;

            Ok(().into())
        },
    )?;

    engine.register_custom_syntax(
        &["rule", "$expr$", "$expr$"], // the custom syntax
        0, // the number of new variables declared within this custom syntax
        |engine, ctx, scope, inputs| {
            let pattern = get_string!(engine, ctx, scope, inputs, 0);
            let settings = get_map!(engine, ctx, scope, inputs, 1);
            let mut rule = Rule::default();

            rule.pattern = Regex::new(&format!("^{}$", pattern)).map_err(|e| e.to_string())?;

            for (key, value) in settings.iter().map(|(k, v)| (k.to_string(), v)) {
                set!(bool, rule, manage, key, value);
                set!(bool, rule, has_custom_titlebar, key, value);
                set!(bool, rule, firefox, key, value);
                set!(bool, rule, chromium, key, value);
                set!(i32, rule, workspace_id, key, value);
            }

            add_rule(engine, scope, rule);

            Ok(().into())
        },
    )?;

    engine.register_custom_syntax(
        &["update_channel", "$expr$", "$expr$"], // the custom syntax
        0, // the number of new variables declared within this custom syntax
        |engine, ctx, scope, inputs| {
            let name = get_string!(engine, ctx, scope, inputs, 0);
            let settings = get_map!(engine, ctx, scope, inputs, 1);
            let mut update_channel = UpdateChannel::default();

            update_channel.name = name;

            for (key, value) in settings.iter().map(|(k, v)| (k.to_string(), v)) {
                set!(String, update_channel, branch, key, value);
                set!(String, update_channel, repo, key, value);
                set!(String, update_channel, version, key, value);
            }

            scope.set_value("__new_update_channel", update_channel);

            engine
                .consume_with_scope(scope, "__update_channels.push(__new_update_channel);")
                .unwrap();

            Ok(().into())
        },
    )?;

    engine.register_custom_syntax(
        &["ignore", "$expr$"], // the custom syntax
        0,                     // the number of new variables declared within this custom syntax
        |engine, ctx, scope, inputs| {
            let pattern = get_string!(engine, ctx, scope, inputs, 0);
            let mut rule = Rule::default();

            rule.pattern = Regex::new(&format!("^{}$", pattern)).map_err(|e| e.to_string())?;
            rule.manage = false;

            add_rule(engine, scope, rule);

            Ok(().into())
        },
    )?;

    engine.register_custom_syntax(
        &["workspace", "$expr$", "$expr$"], // the custom syntax
        0, // the number of new variables declared within this custom syntax
        |engine, ctx, scope, inputs| {
            let id = get_int!(engine, ctx, scope, inputs, 0);
            let settings = get_map!(engine, ctx, scope, inputs, 1);
            let mut workspace = WorkspaceSetting::default();

            workspace.id = id;

            for (key, value) in settings.iter().map(|(k, v)| (k.to_string(), v)) {
                set!(i32, workspace, monitor, key, value);
                set!(String, workspace, text, key, value);
            }

            scope.set_value("__new_workspace_setting", workspace);

            engine
                .consume_with_scope(scope, "__workspace_settings.push(__new_workspace_setting);")
                .unwrap();

            Ok(().into())
        },
    )?;

    engine.register_custom_syntax(
        &["mode", "$expr$", "$expr$", "$block$"], // the custom syntax
        0, // the number of new variables declared within this custom syntax
        |engine, ctx, scope, inputs| {
            let name = get_string!(engine, ctx, scope, inputs, 0);
            let key = get_string!(engine, ctx, scope, inputs, 1);

            add_keybinding(engine, scope, key, KeybindingType::ToggleMode(name.clone()));

            scope.set_value("__mode", Some(name));

            engine.eval_expression_tree(ctx, scope, inputs.get(2).unwrap())?;

            scope.set_value("__mode", None as Option<String>);

            Ok(().into())
        },
    )?;

    Ok(())
}
