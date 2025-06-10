import Dialog from '@mui/material/Dialog'
import DialogContent from '@mui/material/DialogContent'
import DialogTitle from '@mui/material/DialogTitle'
import SwapAmountSelector from './SwapAmountSelector'
import { Alert, Typography, Box, DialogActions, Button } from '@mui/material'
import BitcoinQrCode from 'renderer/components/modal/swap/BitcoinQrCode'
import ActionableMonospaceTextBox from 'renderer/components/other/ActionableMonospaceTextBox'

export default function SwapDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
    return (
        <Dialog open={open} onClose={onClose}>
            <DialogTitle>
                <Typography variant="h2">Swap</Typography>
            </DialogTitle>
            <DialogContent>
                <Box sx={{
                    display: "flex",
                    flexDirection: "column",
                    gap: 2,
                }}>
                    <SwapAmountSelector fullWidth/>
                    <Alert severity="info" variant="outlined">
                        Your Wallet has 0.00000000 BTC. You need an additional
                        0.00000000 BTC to swap your desired XMR amount.
                    </Alert>
                    <Typography variant="h3">Get Bitcoin</Typography>
                    <Typography variant="body1">Send Bitcoin to your internal wallet</Typography>
                    <Box
                        sx={{
                            display: 'flex',
                            flexDirection: 'row',
                            gap: 2,
                        }}
                    >
                        <Box
                            sx={{
                                display: 'flex',
                                flexDirection: 'column',
                                gap: 2,
                                backgroundColor: "white",
                                padding: 2,
                                borderRadius: 2,
                                maxWidth: "200px",
                            }}
                        >
                            <BitcoinQrCode address="1234567890" />
                        </Box>
                        <Box
                            sx={{
                                display: 'flex',
                                flexDirection: 'column',
                                gap: 2,
                            }}
                        >
                            <ActionableMonospaceTextBox content="1234567890" />
                            <ActionableMonospaceTextBox content="1234567890" />
                        </Box>
                    </Box>
                </Box>
            </DialogContent>
                <DialogActions>
                    <Button variant="contained" color="primary">Swap</Button>
                </DialogActions>
        </Dialog>
    )
}
