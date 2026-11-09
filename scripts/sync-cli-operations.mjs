// Keep discoverable CLI inputs aligned with the frontend's typed backend calls.
// Routes are shared data; this script derives contracts, never executes a call.
import fs from 'node:fs';
import ts from 'typescript';

const file = 'src/lib/operations.json';
const entries = JSON.parse(fs.readFileSync(file, 'utf8'));
const cfg = ts.readConfigFile('tsconfig.json', ts.sys.readFile).config;
const parsed = ts.parseJsonConfigFileContent(cfg, ts.sys, process.cwd());
const program = ts.createProgram(parsed.fileNames, parsed.options);
const checker = program.getTypeChecker();
const source = program.getSourceFile('src/lib/api.ts');
function visit(node) {
  if (ts.isCallExpression(node) && node.expression.getText(source) === 'call'
      && node.arguments[0] && ts.isStringLiteral(node.arguments[0])) {
    const entry = entries[node.arguments[0].text];
    if (entry) {
      let expression = node.arguments[1];
      entry.input = (expression?.getText(source) || '{}').replace(/\r\n/g, '\n');
      let fn = node.parent;
      while (fn && !ts.isFunctionDeclaration(fn)) fn = fn.parent;
      if (fn) entry.signature = `${fn.name?.text}(${fn.parameters.map(p => p.getText(source)).join(', ')})`.replace(/\r\n/g, '\n');
      while (expression && (ts.isAsExpression(expression) || ts.isParenthesizedExpression(expression))) expression = expression.expression;
      const properties = expression ? checker.getNonNullableType(checker.getTypeAtLocation(expression)).getProperties() : [];
      const inputs = {};
      for (const symbol of properties) {
        const declaration = symbol.valueDeclaration;
        let optional = !!(symbol.flags & ts.SymbolFlags.Optional);
        // Shorthand properties may originate in defaulted/optional parameters.
        if (declaration && ts.isShorthandPropertyAssignment(declaration)) {
          const origin = checker.getShorthandAssignmentValueSymbol(declaration)?.valueDeclaration;
          if (origin && ts.isParameter(origin)) optional ||= !!(origin.questionToken || origin.initializer);
        }
        const type = checker.getTypeOfSymbolAtLocation(symbol, expression);
        inputs[symbol.name] = {
          type: checker.typeToString(type),
          required: !optional,
        };
        const members = (type.isUnion() ? type.types : [type]).filter(t => !(t.flags & (ts.TypeFlags.Undefined | ts.TypeFlags.Null)));
        if (members.length && members.every(t => !!(t.flags & ts.TypeFlags.StringLiteral))) inputs[symbol.name].enum = members.map(t => t.value);
      }
      if (entry.input.includes('variantArgs(')) inputs.variant = { type: '"cn" | "ai"', required: false, enum:['cn','ai'] };
      entry.parameters = inputs;
    }
  }
  ts.forEachChild(node, visit);
}
visit(source);
// The HTTP adapter intentionally accepts these optional arguments even though
// some desktop wrappers use a path-based export instead of returning content.
entries.export_all_accounts.parameters = {};
entries.get_qoder_companion.parameters = { global: {type:'boolean',required:false} };
entries.session_links_preview.parameters = {targetAccountId:{type:'string',required:true},variant:{type:'"cn" | "ai"',required:false,enum:['cn','ai']}};
entries.checkin_all.parameters = {variant:{type:'"cn" | "ai"',required:false,enum:['cn','ai']},respectWindow:{type:'boolean',required:false}};
const text = `${JSON.stringify(entries, null, 2)}\n`;
if (process.argv.includes('--write')) fs.writeFileSync(file, text);
else if (fs.readFileSync(file, 'utf8') !== text) {
  console.error('CLI operation contracts are stale; run node scripts/sync-cli-operations.mjs --write');
  process.exitCode = 1;
}
