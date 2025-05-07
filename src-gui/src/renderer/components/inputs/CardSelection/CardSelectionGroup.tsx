import { Box } from '@material-ui/core'
import CheckIcon from '@material-ui/icons/Check'
import { CardSelectionProvider } from './CardSelectionContext'

interface CardSelectionGroupProps {
    children: React.ReactElement<{ value: string }>[]
    value: string
    onChange: (value: string) => void
}

export default function CardSelectionGroup({
    children,
    value,
    onChange,
}: CardSelectionGroupProps) {
    return (
        <CardSelectionProvider initialValue={value} onChange={onChange}>
            <Box style={{ display: 'flex', flexDirection: 'column', gap: 12, marginTop: 12 }}>
                {children}
            </Box>
        </CardSelectionProvider>
    )
}
