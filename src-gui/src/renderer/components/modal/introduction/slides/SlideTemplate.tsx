import { makeStyles, Paper, Box, Typography, Button } from '@material-ui/core'

type slideTemplateProps = {
    handleContinue: () => void
    handlePrevious: () => void
    hidePreviousButton?: boolean
    stepLabel?: String
    title: String
    children?: React.ReactNode
    imagePath?: string
    imagePadded?: boolean
    customContinueButtonText?: String
}

const useStyles = makeStyles({
    paper: {
        height: "80%",
        width: "80%",
        display: 'flex',
        justifyContent: 'space-between',
    },
    stepLabel: {
        textTransform: 'uppercase',
    },
    splitImage: {
        height: '100%',
        width: '100%',
        objectFit: 'contain'
    }
})

export default function SlideTemplate({
    handleContinue,
    handlePrevious,
    hidePreviousButton,
    stepLabel,
    title,
    children,
    imagePath,
    imagePadded,
    customContinueButtonText
}: slideTemplateProps) {
    const classes = useStyles()

    return (
        <Paper className={classes.paper}>
            <Box m={3} flex alignContent="center" position="relative" width="50%" flexGrow={1}>
                <Box>
                    {stepLabel && (
                        <Typography
                            variant="overline"
                            className={classes.stepLabel}
                        >
                            {stepLabel}
                        </Typography>
                    )}
                    <Typography variant="h4" style={{ marginBottom: 16 }}>{title}</Typography>
                    {children}
                </Box>
                <Box
                    position="absolute"
                    bottom={0}
                    width="100%"
                    display="flex"
                    justifyContent={
                        hidePreviousButton ? 'flex-end' : 'space-between'
                    }
                >
                    {!hidePreviousButton && (
                        <Button onClick={handlePrevious}>Back</Button>
                    )}
                    <Button
                        onClick={handleContinue}
                        variant="contained"
                        color="primary"
                    >
                        {customContinueButtonText ? customContinueButtonText : 'Next' }
                    </Button>
                </Box>
            </Box>
            {imagePath && (
                <Box
                    bgcolor="#212121"
                    width="50%"
                    display="flex"
                    justifyContent="center"
                    p={imagePadded ? "1.5em" : 0}
                >
                    <img src={imagePath} className={classes.splitImage} />
                </Box>
            )}
        </Paper>
    )
}
