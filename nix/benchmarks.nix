{ lib
, runCommand
, jsonnet-bundler
, cacert
, stdenv
, fetchFromGitHub
, go-jsonnet
, sjsonnet
, jsonnet
, hyperfine
, quick ? false
, jrsonnetVariants
}:

with lib;

let
  jsonnetBench = fetchFromGitHub {
    rev = "v0.19.1";
    owner = "google";
    repo = "jsonnet";
    hash = "sha256-q1MNdbyrx4vvN5woe0o90pPqaNtsZjI5RQ7KJt7rOpU=";
  };
  goJsonnetBench = (fetchFromGitHub {
    owner = "google";
    repo = "go-jsonnet";
    rev = "v0.19.1";
    hash = "sha256-FgQYnas0qkIedRAA8ApZXLzEylg6PS6+8zzl7j+yOeI=";
  }) + "/builtin-benchmarks";
  graalvmBench = fetchFromGitHub {
    owner = "oracle";
    repo = "graal";
    rev = "bc305df3fe587960f7635f0185571500e5988475";
    hash = "sha256-4EKB1b2o4/qtYQ+nqbbs621OJrtjApsAWEBcw5EjrYc=";
  };
  kubePrometheusBench =
    let
      src = fetchFromGitHub {
        owner = "prometheus-operator";
        repo = "kube-prometheus";
        rev = "6a570e3154eac26e32da61d784fbe626da4804df";
        hash = "sha256-s6BK8KQiSjql2w6R+8m5pvPPAcKW+SKjQwqxZHjimFw=";
      };
    in
    runCommand "kube-prometheus-vendor"
      {
        outputHash = "sha256-R60RI/7FQPOHisnzANb34m9WPj5D9FeMVoGOjB19zl8=";
        outputHashMode = "recursive";
        buildInputs = [ cacert ];
      }
      ''
        mkdir -p $out
        cp -r ${src}/* $out/
        cd $out
        mkdir vendor
        ${jsonnet-bundler}/bin/jb install
      '';

  # Removes outsiders from the output
  # Useful when comparing performance of different jrsonnet releases
  skipSlow = if quick then "slow benchmark, but only quick requested" else "";
in
stdenv.mkDerivation {
  name = "benchmarks";
  __impure = true;
  unpackPhase = "true";

  buildInputs = [
    go-jsonnet
    sjsonnet
    jsonnet

    hyperfine
  ];

  installPhase =
    let
      mkBench = { name, path, omitSource ? false, pathIsGenerator ? false, skipScala ? "", skipCpp ? "", skipGo ? "", vendor ? "" }: ''
        set -oux

        echo >> $out
        echo "### ${name}" >> $out
        echo >> $out
        ${optionalString (skipGo != "") ''
          echo "> Note: No results for Go, ${skipGo}" >> $out
          echo >> $out
        ''}
        ${optionalString (skipScala != "") ''
          echo "> Note: No results for Scala, ${skipScala}" >> $out
          echo >> $out
        ''}
        ${optionalString (skipCpp != "") ''
          echo "> Note: No results for C++, ${skipCpp}" >> $out
          echo >> $out
        ''}
        ${optionalString (!quick && !omitSource) ''
          echo "<details>" >> $out
          echo "<summary>Source</summary>" >> $out
          echo >> $out
          echo "\`\`\`jsonnet" >> $out
          ${optionalString pathIsGenerator "echo \"// Generator source\" >> $out"}
          cat ${path} >> $out
          echo >> $out
          echo "\`\`\`" >> $out
          echo "</details>" >> $out
          echo >> $out
        ''}
        path=${path}
        ${optionalString pathIsGenerator ''
          go-jsonnet $path > generated.jsonnet
          path=generated.jsonnet
        ''}
        hyperfine -N -w4 -m20 --output=pipe --style=basic --export-markdown result.md \
          ${concatStringsSep " " (forEach jrsonnetVariants (variant:
            "\"${variant.drv}/bin/jrsonnet $path ${optionalString (vendor != "") "-J${vendor}"}\" -n \"Rust${if variant.name != "" then " (${variant.name})" else ""}\""
          ))} \
          ${optionalString (skipGo == "") "\"go-jsonnet $path ${optionalString (vendor != "") "-J ${vendor}"}\" -n \"Go\""} \
          ${optionalString (skipScala == "") "\"sjsonnet $path ${optionalString (vendor != "") "-J ${vendor}"}\" -n \"Scala\""} \
          ${optionalString (skipCpp == "") "\"jsonnet $path ${optionalString (vendor != "") "-J ${vendor}"}\" -n \"C++\""}
        cat result.md >> $out
      '';
    in
    ''
      touch $out
      ${optionalString (!quick) ''
        cat ${./benchmarks.md} >> $out
        echo >> $out

        echo "<details>" >> $out
        echo "<summary>Tested versions</summary>" >> $out
        echo >> $out
        echo Go: $(go-jsonnet --version) >> $out
        echo >> $out
        echo "\`\`\`" >> $out
        go-jsonnet --help >> $out
        echo "\`\`\`" >> $out
        echo >> $out
        echo C++: $(jsonnet --version) >> $out
        echo >> $out
        echo "\`\`\`" >> $out
        jsonnet --help >> $out
        echo "\`\`\`" >> $out
        echo >> $out
        echo Scala: >> $out
        echo >> $out
        echo "\`\`\`" >> $out
        sjsonnet 2>> $out || true
        echo "\`\`\`" >> $out
        echo >> $out
        echo "</details>" >> $out
        echo >> $out

        echo >> $out
      ''}
      echo "## Real world" >> $out
      ${mkBench {name = "Graalvm CI"; path = "${graalvmBench}/ci.jsonnet"; skipCpp = "takes longer than a hour"; skipGo = skipSlow; skipScala = skipSlow;}}
      ${mkBench {name = "Kube-prometheus manifests"; vendor = "${kubePrometheusBench}/vendor"; path = "${kubePrometheusBench}/example.jsonnet"; skipCpp = skipSlow; skipGo = skipSlow; skipScala = skipSlow;}}

      echo >> $out
      echo "## Benchmarks from C++ jsonnet (/perf_tests)" >> $out
      ${mkBench {name = "Large string join"; path = "${jsonnetBench}/perf_tests/large_string_join.jsonnet"; skipScala = skipSlow;}}
      ${mkBench {name = "Large string template"; omitSource = true; path = "${jsonnetBench}/perf_tests/large_string_template.jsonnet"; skipGo = "fails with os stack size exhausion"; skipCpp = skipSlow; skipScala = skipSlow;}}
      ${mkBench {name = "Realistic 1"; path = "${jsonnetBench}/perf_tests/realistic1.jsonnet"; skipGo = skipSlow; skipCpp = skipSlow; skipScala = skipSlow;}}
      ${mkBench {name = "Realistic 2"; path = "${jsonnetBench}/perf_tests/realistic2.jsonnet"; skipGo = skipSlow; skipCpp = skipSlow; skipScala = skipSlow;}}

      echo >> $out
      echo "## Benchmarks from C++ jsonnet (/benchmarks)" >> $out
      ${mkBench {name = "Tail call"; path = "${jsonnetBench}/benchmarks/bench.01.jsonnet"; skipScala = skipSlow;}}
      ${mkBench {name = "Inheritance recursion"; path = "${jsonnetBench}/benchmarks/bench.02.jsonnet"; skipCpp = skipSlow; skipGo = skipSlow;}}
      ${mkBench {name = "Simple recursive call"; path = "${jsonnetBench}/benchmarks/bench.03.jsonnet"; skipScala = skipSlow; skipGo = skipSlow;}}
      ${mkBench {name = "Foldl string concat"; path = "${jsonnetBench}/benchmarks/bench.04.jsonnet"; skipCpp = skipSlow; skipScala = skipSlow;}}
      ${mkBench {name = "Array sorts"; path = "${jsonnetBench}/benchmarks/bench.06.jsonnet"; skipScala = "std.reverse is not implemented"; skipCpp = skipSlow;}}
      ${mkBench {name = "Lazy array"; path = "${jsonnetBench}/benchmarks/bench.07.jsonnet"; skipGo = skipSlow; skipScala = skipSlow;}}
      ${mkBench {name = "Inheritance function recursion"; path = "${jsonnetBench}/benchmarks/bench.08.jsonnet"; skipCpp = skipSlow; skipScala = skipSlow;}}
      ${mkBench {name = "String strips"; path = "${jsonnetBench}/benchmarks/bench.09.jsonnet"; skipCpp = skipSlow; skipScala = skipSlow;}}
      ${mkBench {name = "Big object"; path = "${jsonnetBench}/benchmarks/gen_big_object.jsonnet"; pathIsGenerator = true; skipScala = skipSlow;}}

      echo >> $out
      echo "## Benchmarks from Go jsonnet (builtins)" >> $out
      ${mkBench {name = "std.base64"; path = "${goJsonnetBench}/base64.jsonnet"; skipCpp = skipSlow; skipScala = skipSlow;}}
      ${mkBench {name = "std.base64Decode"; path = "${goJsonnetBench}/base64Decode.jsonnet"; skipCpp = skipSlow; skipScala = skipSlow;}}
      ${mkBench {name = "std.base64DecodeBytes"; path = "${goJsonnetBench}/base64DecodeBytes.jsonnet"; skipCpp = skipSlow; skipGo = skipSlow; skipScala = skipSlow;}}
      ${mkBench {name = "std.base64 (byte array)"; path = "${goJsonnetBench}/base64_byte_array.jsonnet"; skipCpp = skipSlow; skipGo = skipSlow; skipScala = skipSlow;}}
      ${mkBench {name = "std.foldl"; path = "${goJsonnetBench}/foldl.jsonnet"; skipScala = skipSlow;}}
      ${mkBench {name = "std.manifestJsonEx"; path = "${goJsonnetBench}/manifestJsonEx.jsonnet"; skipScala = skipSlow; skipCpp = skipSlow;}}
      ${mkBench {name = "std.manifestTomlEx"; path = "${goJsonnetBench}/manifestTomlEx.jsonnet"; skipScala = "std.manifestTomlEx is not implemented"; skipCpp=skipSlow;}}
      ${mkBench {name = "std.parseInt"; path = "${goJsonnetBench}/parseInt.jsonnet"; skipScala = skipSlow; skipCpp = skipSlow;}}
      ${mkBench {name = "std.reverse"; path = "${goJsonnetBench}/reverse.jsonnet"; skipScala = "std.reverse is not implemented"; skipCpp = skipSlow; skipGo = skipSlow;}}
      ${mkBench {name = "std.substr"; path = "${goJsonnetBench}/substr.jsonnet"; skipScala = skipSlow;}}
      ${mkBench {name = "Comparsion for array"; path = "${goJsonnetBench}/comparison.jsonnet"; skipScala = "array comparsion is not implemented"; skipCpp = skipSlow;}}
      ${mkBench {name = "Comparsion for primitives"; path = "${goJsonnetBench}/comparison2.jsonnet"; skipCpp = "can't run: uses up to 192GB of RAM"; skipGo = skipSlow; skipScala = skipSlow;}}
    '';
}
