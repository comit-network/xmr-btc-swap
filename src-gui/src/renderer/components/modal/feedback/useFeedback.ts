import { useState, useEffect } from 'react'
import { store } from 'renderer/store/storeRenderer'
import { useActiveSwapInfo } from 'store/hooks'
import { logsToRawString } from 'utils/parseUtils'
import { getLogsOfSwap, redactLogs } from 'renderer/rpc'
import { CliLog, parseCliLogString } from 'models/cliModel'
import logger from 'utils/logger'
import { submitFeedbackViaHttp } from 'renderer/api'
import { addFeedbackId } from 'store/features/conversationsSlice'
import { AttachmentInput } from 'models/apiModel'
import { useSnackbar } from 'notistack'

export const MAX_FEEDBACK_LENGTH = 4000

interface FeedbackInputState {
    bodyText: string
    selectedSwap: string | null
    attachDaemonLogs: boolean
    isSwapLogsRedacted: boolean
    isDaemonLogsRedacted: boolean
}

interface FeedbackLogsState {
    swapLogs: (string | CliLog)[] | null
    daemonLogs: (string | CliLog)[] | null
}

const initialInputState: FeedbackInputState = {
    bodyText: '',
    selectedSwap: null,
    attachDaemonLogs: true,
    isSwapLogsRedacted: false,
    isDaemonLogsRedacted: false,
}

const initialLogsState: FeedbackLogsState = {
    swapLogs: null,
    daemonLogs: null,
}

export function useFeedback() {
    const currentSwapId = useActiveSwapInfo()
    const { enqueueSnackbar } = useSnackbar()

    const [inputState, setInputState] = useState<FeedbackInputState>({
        ...initialInputState,
        selectedSwap: currentSwapId?.swap_id || null,
    })
    const [logsState, setLogsState] =
        useState<FeedbackLogsState>(initialLogsState)
    const [isPending, setIsPending] = useState(false)
    const [error, setError] = useState<string | null>(null)

    const bodyTooLong = inputState.bodyText.length > MAX_FEEDBACK_LENGTH

    useEffect(() => {
        if (inputState.selectedSwap === null) {
            setLogsState((prev) => ({ ...prev, swapLogs: null }))
            return
        }

        getLogsOfSwap(inputState.selectedSwap, inputState.isSwapLogsRedacted)
            .then((response) => {
                setLogsState((prev) => ({
                    ...prev,
                    swapLogs: response.logs.map(parseCliLogString),
                }))
                setError(null)
            })
            .catch((e) => {
                logger.error(`Failed to fetch swap logs: ${e}`)
                setLogsState((prev) => ({ ...prev, swapLogs: null }))
                setError(`Failed to fetch swap logs: ${e}`)
            })
    }, [inputState.selectedSwap, inputState.isSwapLogsRedacted])

    useEffect(() => {
        if (!inputState.attachDaemonLogs) {
            setLogsState((prev) => ({ ...prev, daemonLogs: null }))
            return
        }

        try {
            if (inputState.isDaemonLogsRedacted) {
                redactLogs(store.getState().rpc?.logs)
                    .then((redactedLogs) => {
                        setLogsState((prev) => ({
                            ...prev,
                            daemonLogs: redactedLogs,
                        }))
                        setError(null)
                    })
                    .catch((e) => {
                        logger.error(`Failed to redact daemon logs: ${e}`)
                        setLogsState((prev) => ({ ...prev, daemonLogs: null }))
                        setError(`Failed to redact daemon logs: ${e}`)
                    })
            } else {
                setLogsState((prev) => ({
                    ...prev,
                    daemonLogs: store.getState().rpc?.logs,
                }))
                setError(null)
            }
        } catch (e) {
            logger.error(`Failed to fetch daemon logs: ${e}`)
            setLogsState((prev) => ({ ...prev, daemonLogs: null }))
            setError(`Failed to fetch daemon logs: ${e}`)
        }
    }, [inputState.attachDaemonLogs, inputState.isDaemonLogsRedacted])

    const clearState = () => {
        setInputState(initialInputState)
        setLogsState(initialLogsState)
        setError(null)
    }

    const submitFeedback = async () => {
        if (inputState.bodyText.length === 0) {
            setError('Please enter a message')
            throw new Error('User did not enter a message')
        }

        const attachments: AttachmentInput[] = []
        // Add swap logs as an attachment
        if (logsState.swapLogs) {
            attachments.push({
                key: `swap_logs_${inputState.selectedSwap}.txt`,
                content: logsToRawString(logsState.swapLogs),
            })
        }

        // Handle daemon logs
        if (logsState.daemonLogs) {
            attachments.push({
                key: 'daemon_logs.txt',
                content: logsToRawString(logsState.daemonLogs),
            })
        }

        // Call the updated API function
        const feedbackId = await submitFeedbackViaHttp(
            inputState.bodyText,
            attachments
        )

        enqueueSnackbar('Feedback submitted successfully', {
            variant: 'success',
        })

        // Dispatch only the ID
        store.dispatch(addFeedbackId(feedbackId))
    }

    return {
        input: inputState,
        setInputState,
        logs: logsState,
        error,
        clearState,
        submitFeedback,
    }
}
