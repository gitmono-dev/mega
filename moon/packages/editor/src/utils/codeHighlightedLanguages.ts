import { refractor } from 'refractor'
import bash from 'refractor/bash'
import clike from 'refractor/clike'
import clojure from 'refractor/clojure'
import cpp from 'refractor/cpp'
import csharp from 'refractor/csharp'
import css from 'refractor/css'
import elixir from 'refractor/elixir'
import erlang from 'refractor/erlang'
import go from 'refractor/go'
import graphql from 'refractor/graphql'
import groovy from 'refractor/groovy'
import haskell from 'refractor/haskell'
import hcl from 'refractor/hcl'
import ini from 'refractor/ini'
import java from 'refractor/java'
import javascript from 'refractor/javascript'
import json from 'refractor/json'
import jsx from 'refractor/jsx'
import kotlin from 'refractor/kotlin'
import lisp from 'refractor/lisp'
import lua from 'refractor/lua'
import markup from 'refractor/markup'
import nix from 'refractor/nix'
import objectivec from 'refractor/objectivec'
import ocaml from 'refractor/ocaml'
import perl from 'refractor/perl'
import php from 'refractor/php'
import powershell from 'refractor/powershell'
import python from 'refractor/python'
import ruby from 'refractor/ruby'
import rust from 'refractor/rust'
import sass from 'refractor/sass'
import scala from 'refractor/scala'
import scss from 'refractor/scss'
import solidity from 'refractor/solidity'
import sql from 'refractor/sql'
import swift from 'refractor/swift'
import toml from 'refractor/toml'
import tsx from 'refractor/tsx'
import typescript from 'refractor/typescript'
import verilog from 'refractor/verilog'
import vhdl from 'refractor/vhdl'
import visualbasic from 'refractor/visual-basic'
import yaml from 'refractor/yaml'
import zig from 'refractor/zig'

export const ALIAS_TO_LANGUAGE = [
  [bash, 'Bash'] as const,
  [clojure, 'Clojure'] as const,
  [cpp, 'C++'] as const,
  [css, 'CSS'] as const,
  [clike, 'C-Like'] as const,
  [csharp, 'C#'] as const,
  [elixir, 'Elixir'] as const,
  [erlang, 'Erlang'] as const,
  [go, 'Go'] as const,
  [graphql, 'GraphQL'] as const,
  [groovy, 'Groovy'] as const,
  [haskell, 'Haskell'] as const,
  [hcl, 'HCL'] as const,
  [ini, 'INI'] as const,
  [java, 'Java'] as const,
  [javascript, 'JavaScript'] as const,
  [jsx, 'JSX'] as const,
  [json, 'JSON'] as const,
  [kotlin, 'Kotlin'] as const,
  [lisp, 'Lisp'] as const,
  [lua, 'Lua'] as const,
  [markup, 'HTML'] as const,
  [nix, 'Nix'] as const,
  [objectivec, 'Objective-C'] as const,
  [ocaml, 'OCaml'] as const,
  [perl, 'Perl'] as const,
  [php, 'PHP'] as const,
  [python, 'Python'] as const,
  [powershell, 'PowerShell'] as const,
  [ruby, 'Ruby'] as const,
  [rust, 'Rust'] as const,
  [scala, 'Scala'] as const,
  [sql, 'SQL'] as const,
  [solidity, 'Solidity'] as const,
  [sass, 'Sass'] as const,
  [scss, 'SCSS'] as const,
  [swift, 'Swift'] as const,
  [toml, 'TOML'] as const,
  [typescript, 'TypeScript'] as const,
  [tsx, 'TSX'] as const,
  [verilog, 'Verilog'] as const,
  [vhdl, 'VHDL'] as const,
  [visualbasic, 'Visual Basic'] as const,
  [yaml, 'YAML'] as const,
  [zig, 'Zig'] as const
].reduce(
  (acc, [language, name]) => {
    refractor.register(language)
    acc[language.displayName] = name
    language.aliases.forEach((alias) => {
      acc[alias] = name
    })
    return acc
  },
  { none: 'Plain Text' } as Record<string, string>
)
