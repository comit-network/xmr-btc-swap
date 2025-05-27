import { getLogsOfSwap, saveLogFiles } from 'renderer/rpc'
import PromiseInvokeButton from 'renderer/components/PromiseInvokeButton'
import { store } from 'renderer/store/storeRenderer'
import { ButtonProps } from '@material-ui/core'
import { logsToRawString } from 'utils/parseUtils'

interface ExportLogsButtonProps extends ButtonProps {
    swap_id: string
}

export default function ExportLogsButton({ swap_id, ...buttonProps }: ExportLogsButtonProps) {
    async function handleExportLogs() {
        const swapLogs = await getLogsOfSwap(swap_id, false)
        const daemonLogs = store.getState().rpc?.logs

        const logContent = {
            swap_logs: logsToRawString(swapLogs.logs),
            daemon_logs: logsToRawString(daemonLogs),
        }

        await saveLogFiles(
            `swap_${swap_id}_logs.zip`,
            logContent
        )
    }

    return (
        <PromiseInvokeButton 
            onInvoke={handleExportLogs} 
            {...buttonProps}
        >
            Export Logs
        </PromiseInvokeButton>
    )
}
