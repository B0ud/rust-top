function calculateDaysBetweenDates(begin, end) {
    const beginDate = new Date(begin);
    const endDate = new Date(end);
    const difference = endDate.getTime() - beginDate.getTime();
    const days = difference / (1000 * 3600 * 24);
    return days;
}       
