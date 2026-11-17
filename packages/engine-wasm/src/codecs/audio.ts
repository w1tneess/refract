import { RefractError, type Inspect, type Item, type Options } from '@refract/core';
import type { Ctx } from '../protocol.ts';

// Filled in by milestone M3 (audio tools).
const todo = () => new RefractError('Unsupported', 'Audio tools are not built yet', 'Try again in a later version.');
export async function run(_op: string, _items: Item[], _opts: Options, _ctx: Ctx): Promise<Item[]> { throw todo(); }
export async function inspect(_item: Item, _ctx: Ctx): Promise<Inspect> { throw todo(); }
