import { Typography, Box, DialogContent, DialogActions, Button } from '@mui/material'

export default function SelectMaker({ onNext, onBack }: { onNext: () => void; onBack: () => void }) {
    return (
        <>
            <DialogContent>
                <Box sx={{
                    display: "flex",
                    flexDirection: "column",
                    gap: 2,
                }}>
                    <Typography variant="h3">Select a Maker</Typography>
                    {/* Add maker selection UI here */}
                </Box>
            </DialogContent>
            <DialogActions>
                <Button variant="outlined" onClick={onBack}>
                    Back
                </Button>
                <Button variant="contained" color="primary" onClick={onNext}>
                    Next
                </Button>
            </DialogActions>
        </>
    )
} 