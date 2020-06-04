use crate::{
	binding, context_creator, create_error, function_default, function_rhs, future_wrapper,
	lazy_binding, lazy_val, push, Context, ContextCreator, FuncDesc, LazyBinding, ObjMember,
	ObjValue, Result, Val,
};
use closure::closure;
use jsonnet_parser::{
	el, Arg, ArgsDesc, AssertStmt, BinaryOpType, BindSpec, CompSpec, Expr, FieldMember,
	ForSpecData, IfSpecData, LiteralType, LocExpr, Member, ObjBody, ParamsDesc, UnaryOpType,
	Visibility,
};
use std::{
	collections::{BTreeMap, HashMap},
	rc::Rc,
};

pub fn evaluate_binding(b: &BindSpec, context_creator: ContextCreator) -> (String, LazyBinding) {
	let b = b.clone();
	if let Some(args) = &b.params {
		let args = args.clone();
		(
			b.name.clone(),
			lazy_binding!(move |this, super_obj| Ok(lazy_val!(
				closure!(clone b, clone args, clone context_creator, || Ok(evaluate_method(
					context_creator.0(this.clone(), super_obj.clone())?,
					&b.value,
					args.clone()
				)))
			))),
		)
	} else {
		(
			b.name.clone(),
			lazy_binding!(move |this, super_obj| {
				Ok(lazy_val!(
					closure!(clone context_creator, clone b, || evaluate(
						context_creator.0(this.clone(), super_obj.clone())?,
						&b.value
					))
				))
			}),
		)
	}
}

pub fn evaluate_method(ctx: Context, expr: &LocExpr, arg_spec: ParamsDesc) -> Val {
	Val::Func(FuncDesc {
		ctx,
		params: arg_spec,
		eval_rhs: function_rhs!(closure!(clone expr, |ctx| evaluate(ctx, &expr))),
		eval_default: function_default!(closure!(|ctx, default| evaluate(ctx, &default))),
	})
}

pub fn evaluate_field_name(
	context: Context,
	field_name: &jsonnet_parser::FieldName,
) -> Result<String> {
	Ok(match field_name {
		jsonnet_parser::FieldName::Fixed(n) => n.clone(),
		jsonnet_parser::FieldName::Dyn(expr) => {
			evaluate(context, expr)?.try_cast_str("dynamic field name")?
		}
	})
}

pub fn evaluate_unary_op(op: UnaryOpType, b: &Val) -> Result<Val> {
	Ok(match (op, b) {
		(o, Val::Lazy(l)) => evaluate_unary_op(o, &l.evaluate()?)?,
		(UnaryOpType::Not, Val::Bool(v)) => Val::Bool(!v),
		(op, o) => panic!("unary op not implemented: {:?} {:?}", op, o),
	})
}

pub(crate) fn evaluate_add_op(a: &Val, b: &Val) -> Result<Val> {
	Ok(match (a, b) {
		(Val::Str(v1), Val::Str(v2)) => Val::Str(v1.to_owned() + &v2),
		(Val::Str(v1), Val::Num(v2)) => Val::Str(format!("{}{}", v1, v2)),
		(Val::Num(v1), Val::Str(v2)) => Val::Str(format!("{}{}", v1, v2)),
		(Val::Str(v1), v2) => Val::Str(format!("{}{:?}", v1, v2)),
		(Val::Obj(v1), Val::Obj(v2)) => Val::Obj(v2.with_super(v1.clone())),
		(Val::Arr(a), Val::Arr(b)) => Val::Arr([&a[..], &b[..]].concat()),
		(Val::Num(v1), Val::Num(v2)) => Val::Num(v1 + v2),
		_ => panic!("can't add: {:?} and {:?}", a, b),
	})
}

pub fn evaluate_binary_op(context: Context, a: &Val, op: BinaryOpType, b: &Val) -> Result<Val> {
	Ok(match (a, op, b) {
		(Val::Lazy(a), o, b) => evaluate_binary_op(context, &a.evaluate()?, o, b)?,
		(a, o, Val::Lazy(b)) => evaluate_binary_op(context, a, o, &b.evaluate()?)?,

		(a, BinaryOpType::Add, b) => evaluate_add_op(a, b)?,

		(Val::Str(v1), BinaryOpType::Ne, Val::Str(v2)) => Val::Bool(v1 != v2),

		(Val::Str(v1), BinaryOpType::Mul, Val::Num(v2)) => Val::Str(v1.repeat(*v2 as usize)),
		(Val::Str(format), BinaryOpType::Mod, args) => evaluate(
			context
				.with_var("__tmp__format__".to_owned(), Val::Str(format.to_owned()))?
				.with_var(
					"__tmp__args__".to_owned(),
					match args {
						Val::Arr(v) => Val::Arr(v.clone()),
						v => Val::Arr(vec![v.clone()]),
					},
				)?,
			&el!(Expr::Apply(
				el!(Expr::Index(
					el!(Expr::Var("std".to_owned())),
					el!(Expr::Str("format".to_owned()))
				)),
				ArgsDesc(vec![
					Arg(None, el!(Expr::Var("__tmp__format__".to_owned()))),
					Arg(None, el!(Expr::Var("__tmp__args__".to_owned())))
				])
			)),
		)?,

		(Val::Bool(a), BinaryOpType::And, Val::Bool(b)) => Val::Bool(*a && *b),
		(Val::Bool(a), BinaryOpType::Or, Val::Bool(b)) => Val::Bool(*a || *b),

		(Val::Num(v1), BinaryOpType::Mul, Val::Num(v2)) => Val::Num(v1 * v2),
		(Val::Num(v1), BinaryOpType::Div, Val::Num(v2)) => Val::Num(v1 / v2),
		(Val::Num(v1), BinaryOpType::Mod, Val::Num(v2)) => Val::Num(v1 % v2),

		(Val::Num(v1), BinaryOpType::Sub, Val::Num(v2)) => Val::Num(v1 - v2),

		(Val::Num(v1), BinaryOpType::Lhs, Val::Num(v2)) => {
			Val::Num(((*v1 as i32) << (*v2 as i32)) as f64)
		}
		(Val::Num(v1), BinaryOpType::Rhs, Val::Num(v2)) => {
			Val::Num(((*v1 as i32) >> (*v2 as i32)) as f64)
		}

		(Val::Num(v1), BinaryOpType::Lt, Val::Num(v2)) => Val::Bool(v1 < v2),
		(Val::Num(v1), BinaryOpType::Gt, Val::Num(v2)) => Val::Bool(v1 > v2),
		(Val::Num(v1), BinaryOpType::Lte, Val::Num(v2)) => Val::Bool(v1 <= v2),
		(Val::Num(v1), BinaryOpType::Gte, Val::Num(v2)) => Val::Bool(v1 >= v2),

		(Val::Num(v1), BinaryOpType::Eq, Val::Num(v2)) => Val::Bool((v1 - v2).abs() < f64::EPSILON),
		(Val::Num(v1), BinaryOpType::Ne, Val::Num(v2)) => Val::Bool((v1 - v2).abs() > f64::EPSILON),

		(Val::Num(v1), BinaryOpType::BitAnd, Val::Num(v2)) => {
			Val::Num(((*v1 as i32) & (*v2 as i32)) as f64)
		}
		(Val::Num(v1), BinaryOpType::BitOr, Val::Num(v2)) => {
			Val::Num(((*v1 as i32) | (*v2 as i32)) as f64)
		}
		(Val::Num(v1), BinaryOpType::BitXor, Val::Num(v2)) => {
			Val::Num(((*v1 as i32) ^ (*v2 as i32)) as f64)
		}
		(a, BinaryOpType::Eq, b) => Val::Bool(a == b),
		(a, BinaryOpType::Ne, b) => Val::Bool(a != b),
		_ => panic!("no rules for binary operation: {:?} {:?} {:?}", a, op, b),
	})
}

future_wrapper!(HashMap<String, LazyBinding>, FutureNewBindings);
future_wrapper!(ObjValue, FutureObjValue);

pub fn evaluate_comp(
	context: Context,
	value: &LocExpr,
	specs: &[CompSpec],
) -> Result<Option<Vec<Val>>> {
	Ok(match specs.get(0) {
		None => Some(vec![evaluate(context, &value)?]),
		Some(CompSpec::IfSpec(IfSpecData(cond))) => {
			if evaluate(context.clone(), &cond)?.try_cast_bool("if spec")? {
				evaluate_comp(context, value, &specs[1..])?
			} else {
				None
			}
		}
		Some(CompSpec::ForSpec(ForSpecData(var, expr))) => {
			match evaluate(context.clone(), &expr)?.unwrap_if_lazy()? {
				Val::Arr(list) => {
					let mut out = Vec::new();
					for item in list {
						let item = item.clone();
						out.push(evaluate_comp(
							context.with_var(var.clone(), item)?,
							value,
							&specs[1..],
						)?);
					}
					Some(out.iter().flatten().flatten().cloned().collect())
				}
				_ => panic!("for expression evaluated to non-iterable value"),
			}
		}
	})
}

// TODO: Asserts
pub fn evaluate_object(context: Context, object: ObjBody) -> Result<ObjValue> {
	Ok(match object {
		ObjBody::MemberList(members) => {
			let new_bindings = FutureNewBindings::new();
			let future_this = FutureObjValue::new();
			let context_creator = context_creator!(
				closure!(clone context, clone new_bindings, clone future_this, |this: Option<ObjValue>, super_obj: Option<ObjValue>| {
					Ok(context.clone().extend(
						new_bindings.clone().unwrap(),
						context.clone().dollar().clone().or_else(||this.clone()),
						Some(this.unwrap()),
						super_obj
					)?)
				})
			);
			{
				let mut bindings: HashMap<String, LazyBinding> = HashMap::new();
				for (n, b) in members
					.iter()
					.filter_map(|m| match m {
						Member::BindStmt(b) => Some(b.clone()),
						_ => None,
					})
					.map(|b| evaluate_binding(&b, context_creator.clone()))
				{
					bindings.insert(n, b);
				}
				new_bindings.fill(bindings);
			}

			let mut new_members = BTreeMap::new();
			for member in members.into_iter() {
				match member {
					Member::Field(FieldMember {
						name,
						plus,
						params: None,
						visibility,
						value,
					}) => {
						let name = evaluate_field_name(context.clone(), &name)?;
						new_members.insert(
							name.clone(),
							ObjMember {
								add: plus,
								visibility: visibility.clone(),
								invoke: binding!(
									closure!(clone name, clone value, clone context_creator, |this, super_obj| {
										push(value.clone(), "object ".to_owned()+&name+" field", ||{
											let context = context_creator.0(this, super_obj)?;
											evaluate(
												context,
												&value,
											)?.unwrap_if_lazy()
										})
									})
								),
							},
						);
					}
					Member::Field(FieldMember {
						name,
						params: Some(params),
						value,
						..
					}) => {
						let name = evaluate_field_name(context.clone(), &name)?;
						new_members.insert(
							name,
							ObjMember {
								add: false,
								visibility: Visibility::Hidden,
								invoke: binding!(
									closure!(clone value, clone context_creator, |this, super_obj| {
										// TODO: Assert
										Ok(evaluate_method(
											context_creator.0(this, super_obj)?,
											&value.clone(),
											params.clone(),
										))
									})
								),
							},
						);
					}
					Member::BindStmt(_) => {}
					Member::AssertStmt(_) => {}
				}
			}
			future_this.fill(ObjValue::new(None, Rc::new(new_members)))
		}
		_ => todo!(),
	})
}

pub fn evaluate(context: Context, expr: &LocExpr) -> Result<Val> {
	use Expr::*;
	let locexpr = expr.clone();
	let LocExpr(expr, loc) = expr;
	Ok(match &**expr {
		Literal(LiteralType::This) => Val::Obj(
			context
				.this()
				.clone()
				.unwrap_or_else(|| panic!("this not found")),
		),
		Literal(LiteralType::Super) => Val::Obj(
			context
				.super_obj()
				.clone()
				.unwrap_or_else(|| panic!("super not found")),
		),
		Literal(LiteralType::True) => Val::Bool(true),
		Literal(LiteralType::False) => Val::Bool(false),
		Literal(LiteralType::Null) => Val::Null,
		Parened(e) => evaluate(context, e)?,
		Str(v) => Val::Str(v.clone()),
		Num(v) => Val::Num(*v),
		BinaryOp(v1, o, v2) => {
			let a = evaluate(context.clone(), v1)?.unwrap_if_lazy()?;
			let op = *o;
			let b = evaluate(context.clone(), v2)?.unwrap_if_lazy()?;
			evaluate_binary_op(context, &a, op, &b)?
		}
		UnaryOp(o, v) => evaluate_unary_op(*o, &evaluate(context, v)?)?,
		Var(name) => Val::Lazy(context.binding(&name)).unwrap_if_lazy()?,
		Index(value, index) => {
			match (
				evaluate(context.clone(), value)?.unwrap_if_lazy()?,
				evaluate(context.clone(), index)?,
			) {
				(Val::Obj(v), Val::Str(s)) => {
					if let Some(v) = v.get(&s)? {
						v.unwrap_if_lazy()?
					} else if let Some(Val::Str(n)) = v.get("__intristic_namespace__")? {
						Val::Intristic(n, s)
					} else {
						create_error(crate::Error::NoSuchField(s))?
					}
				}
				(Val::Arr(v), Val::Num(n)) => v
					.get(n as usize)
					.unwrap_or_else(|| panic!("out of bounds"))
					.clone(),
				(Val::Str(s), Val::Num(n)) => {
					Val::Str(s.chars().skip(n as usize).take(1).collect())
				}
				(v, i) => todo!("not implemented: {:?}[{:?}]", v, i.unwrap_if_lazy()),
			}
		}
		LocalExpr(bindings, returned) => {
			let mut new_bindings: HashMap<String, LazyBinding> = HashMap::new();
			let future_context = Context::new_future();

			let context_creator = context_creator!(
				closure!(clone future_context, |_, _| Ok(future_context.clone().unwrap()))
			);

			for (k, v) in bindings
				.iter()
				.map(|b| evaluate_binding(b, context_creator.clone()))
			{
				new_bindings.insert(k, v);
			}

			let context = context
				.extend(new_bindings, None, None, None)?
				.into_future(future_context);
			evaluate(context, &returned.clone())?
		}
		Arr(items) => {
			let mut out = Vec::with_capacity(items.len());
			for item in items {
				out.push(evaluate(context.clone(), item)?);
			}
			Val::Arr(out)
		}
		ArrComp(expr, compspecs) => Val::Arr(
			// First compspec should be forspec, so no "None" possible here
			evaluate_comp(context, expr, compspecs)?.unwrap(),
		),
		Obj(body) => Val::Obj(evaluate_object(context, body.clone())?),
		Apply(value, ArgsDesc(args)) => {
			let value = evaluate(context.clone(), value)?.unwrap_if_lazy()?;
			match value {
				Val::Intristic(ns, name) => match (&ns as &str, &name as &str) {
					// arr/string/function
					("std", "length") => {
						assert_eq!(args.len(), 1);
						let expr = &args.get(0).unwrap().1;
						match evaluate(context, expr)? {
							Val::Str(n) => Val::Num(n.chars().count() as f64),
							Val::Arr(i) => Val::Num(i.len() as f64),
							v => panic!("can't get length of {:?}", v),
						}
					}
					// any
					("std", "type") => {
						assert_eq!(args.len(), 1);
						let expr = &args.get(0).unwrap().1;
						Val::Str(evaluate(context, expr)?.value_type()?.name().to_owned())
					}
					// length, idx=>any
					("std", "makeArray") => {
						assert_eq!(args.len(), 2);
						if let (Val::Num(v), Val::Func(d)) = (
							evaluate(context.clone(), &args[0].1)?,
							evaluate(context, &args[1].1)?,
						) {
							assert!(v > 0.0);
							let mut out = Vec::with_capacity(v as usize);
							for i in 0..v as usize {
								out.push(d.evaluate(vec![(None, Val::Num(i as f64))])?)
							}
							Val::Arr(out)
						} else {
							panic!("bad makeArray call");
						}
					}
					// string
					("std", "codepoint") => {
						assert_eq!(args.len(), 1);
						if let Val::Str(s) = evaluate(context, &args[0].1)? {
							assert!(
								s.chars().count() == 1,
								"std.codepoint should receive single char string"
							);
							Val::Num(s.chars().take(1).next().unwrap() as u32 as f64)
						} else {
							panic!("bad codepoint call");
						}
					}
					// object, includeHidden
					("std", "objectFieldsEx") => {
						assert_eq!(args.len(), 2);
						if let (Val::Obj(body), Val::Bool(_include_hidden)) = (
							evaluate(context.clone(), &args[0].1)?,
							evaluate(context, &args[1].1)?,
						) {
							// TODO: handle visibility (_include_hidden)
							Val::Arr(body.fields().into_iter().map(Val::Str).collect())
						} else {
							panic!("bad objectFieldsEx call");
						}
					}
					(ns, name) => panic!("Intristic not found: {}.{}", ns, name),
				},
				Val::Func(f) => push(locexpr.clone(), "function call".to_owned(), || {
					f.evaluate(
						args.clone()
							.into_iter()
							.map(move |a| {
								(
									a.clone().0,
									Val::Lazy(lazy_val!(
										closure!(clone context, clone a, || evaluate(context.clone(), &a.clone().1))
									)),
								)
							})
							.collect(),
					)
				})?,
				_ => panic!("{:?} is not a function", value),
			}
		}
		Function(params, body) => evaluate_method(context, body, params.clone()),
		AssertExpr(AssertStmt(value, msg), returned) => {
			if push(value.clone(), "assertion condition".to_owned(), || {
				evaluate(context.clone(), &value)?
					.try_cast_bool("assertion condition should be boolean")
			})? {
				push(
					returned.clone(),
					"assert 'return' branch".to_owned(),
					|| evaluate(context, returned),
				)?
			} else if let Some(msg) = msg {
				panic!(
					"assertion failed ({:?}): {}",
					value,
					evaluate(context, msg)?.try_cast_str("assertion message should be string")?
				);
			} else {
				panic!("assertion failed ({:?}): no message", value);
			}
		}
		Error(e) => create_error(crate::Error::RuntimeError(
			evaluate(context, e)?.try_cast_str("error text should be string")?,
		))?,
		IfElse {
			cond,
			cond_then,
			cond_else,
		} => {
			if push(cond.0.clone(), "if condition".to_owned(), || {
				evaluate(context.clone(), &cond.0)?.try_cast_bool("if condition should be boolean")
			})? {
				push(
					cond_then.clone(),
					"if condition 'then' branch".to_owned(),
					|| evaluate(context, cond_then),
				)?
			} else {
				match cond_else {
					Some(v) => push(v.clone(), "if condition 'else' branch".to_owned(), || {
						evaluate(context, v)
					})?,
					None => Val::Bool(false),
				}
			}
		}
		_ => panic!(
			"evaluation not implemented: {:?}",
			LocExpr(expr.clone(), loc.clone())
		),
	})
}