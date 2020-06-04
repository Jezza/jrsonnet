use clap::Clap;
use jsonnet_evaluator::Val;

#[derive(Clap)]
#[clap(version = "0.1.0", author = "Lach <iam@lach.pw>")]
struct Opts {
	#[clap(about = "File to compile")]
	input: String,
	#[clap(long, about = "Disable global std variable")]
	no_stdlib: bool,
}

fn main() {
	let opts: Opts = Opts::parse();
	let evaluator = jsonnet_evaluator::EvaluationState::default();
	if !opts.no_stdlib {
		evaluator.add_stdlib();
	}
	evaluator
		.add_file(
			opts.input.clone(),
			String::from_utf8(std::fs::read(opts.input.clone()).unwrap()).unwrap(),
		)
		.unwrap();
	let result = evaluator.evaluate_file(&opts.input);
	match result {
		Ok(v) => match v {
			Val::Str(s) => println!("{}", s),
			Val::Num(n) => println!("{}", n),
			_v => eprintln!(
				"jsonnet output is not a string.\nDid you forgot to set --format, or wrap your data with std.manifestJson?"
			),
		},
		Err(err) => {
			println!("Error: {:?}", err.0);
			use annotate_snippets::{
				display_list::{DisplayList, FormatOptions},
				snippet::{Annotation, AnnotationType, Slice, Snippet, SourceAnnotation},
			};
			for item in (err.1).0.iter() {
				let desc = &item.1;
				if (item.0).1.is_none() {
					continue;
				}
				let source = (item.0).1.clone().unwrap();
				let code = evaluator.get_source(&source.0);
				let snippet = Snippet {
					opt: FormatOptions {
						color: true,
						..Default::default()
					},
					title: Some(Annotation {
						label: Some(&item.1),
						id: None,
						annotation_type: AnnotationType::Error,
					}),
					footer: vec![],
					slices: vec![Slice {
						source: &code,
						line_start: 1,
						origin: Some(&source.0),
						fold: true,
						annotations: vec![SourceAnnotation {
							label: desc,
							annotation_type: AnnotationType::Error,
							range: (source.1, source.2),
						}],
					}],
				};

				let dl = DisplayList::from(snippet);
				println!("{}", dl);
			}
		}
	}
}