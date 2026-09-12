import { createWorkbench } from '@dev-workbench/core'
import { nativeBridge } from './nativeBridge'
export const workbench = createWorkbench(nativeBridge)

