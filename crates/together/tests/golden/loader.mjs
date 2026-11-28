// Lets Node run the studio's TypeScript as it is: imports without ".ts" found, and each
// file turned into JavaScript by the studio's own TypeScript (types dropped, nothing else).
import { registerHooks, createRequire } from 'node:module';
import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath, pathToFileURL } from 'node:url';

const studio = new URL('../../../../apps/studio/', import.meta.url);
const ts = createRequire(new URL('package.json', studio))('typescript');

registerHooks({
	resolve(spec, ctx, next) {
		if ((spec.startsWith('.') || spec.startsWith('/')) && !/\.[cm]?[jt]s$/.test(spec)) {
			const url = new URL(spec + '.ts', ctx.parentURL);
			if (existsSync(fileURLToPath(url))) return next(url.href, ctx);
		}
		// (the paint package, as the studio's bundler finds it)
		if (spec.startsWith('@pencilpilot/paint/'))
			return next(
				pathToFileURL(fileURLToPath(new URL(`../../packages/paint/src/${spec.slice(19)}.ts`, studio))).href,
				ctx
			);
		return next(spec, ctx);
	},
	load(url, ctx, next) {
		if (!url.endsWith('.ts')) return next(url, ctx);
		const source = ts.transpileModule(readFileSync(fileURLToPath(url), 'utf8'), {
			compilerOptions: {
				module: ts.ModuleKind.ESNext,
				target: ts.ScriptTarget.ES2022,
				useDefineForClassFields: true,
				verbatimModuleSyntax: false
			},
			fileName: fileURLToPath(url)
		}).outputText;
		return { format: 'module', source, shortCircuit: true };
	}
});
